#![allow(unsafe_op_in_unsafe_fn)]
use super::{
    editor_theme as theme, error,
    pickers::{self, Pick},
    render, wide,
};
use crate::{
    config::{
        Binding, Browser, Config, CursorEnd, Instrument, ItemIcon, Mode, Node, Profile, Target,
    },
    editing::{self, History},
    editor_layout::{Dpi, Layout as FormLayout, Rect as FormRect},
    geometry::Point,
    storage,
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    io::Write,
    mem::{size_of, zeroed},
    path::PathBuf,
    ptr::{null, null_mut},
    sync::Arc,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{Com::*, LibraryLoader::GetModuleHandleW},
    UI::{Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

// windows-sys omits the TBM_GETPOS alias (WM_USER + 0).
const TBM_GETPOS: u32 = WM_USER;
const TREE: i32 = 100;
const ITEM_PATH: i32 = 223;
const BROWSER: i32 = 225;
const RENAME: i32 = 309;
const REFRESH_TREE: u32 = WM_APP + 40;
const ADVICE_READY: u32 = WM_APP + 60;
const PREVIEW_READY: u32 = WM_APP + 63;
const BROWSE_TARGET: i32 = 211;
const BROWSE_RUNNER: i32 = 212;
const BROWSE_CWD: i32 = 213;
const ADVICE: i32 = 214;
const ICON_MODE: i32 = 215;
const ICON_BROWSE: i32 = 216;
const UNDO: i32 = 405;
const REDO: i32 = 406;
const IMPORT: i32 = 407;
const EXPORT: i32 = 408;
const NAME: i32 = 201;
const NOTE: i32 = 217;
const NOTE_COUNT: i32 = 218;
const KIND: i32 = 202;
const TARGET: i32 = 203;
const ARGS: i32 = 204;
const RUNNER: i32 = 205;
const CWD: i32 = 206;
const PARENT: i32 = 207;
const UNIVERSE: i32 = 219;
const VISIBLE: i32 = 220;
const STARTUP: i32 = 221;
const START_MENU: i32 = 222;
const ADD_UNIVERSE: i32 = 307;
const APPLY: i32 = 301;
const ADD_STAR: i32 = 302;
const ADD_FOLDER: i32 = 303;
const DUPLICATE: i32 = 308;
const DELETE: i32 = 304;
const UP: i32 = 305;
const DOWN: i32 = 306;
const SAVE: i32 = 401;
const RELOAD: i32 = 402;
const ADVANCED: i32 = 403;
const PROFILES: i32 = 404;
const TOOLS: i32 = 409;
const RECOVER_DRAFT: i32 = 704;
const DRAFT_TIMER: usize = 54;
const JSON_TEXT: i32 = 410;
const JSON_SAVE: i32 = 411;
const RADIUS: i32 = 501;
const DISTANCE: i32 = 502;
const SPREAD: i32 = 503;
const GAP: i32 = 504;
const VOLUME: i32 = 506;
const MONITOR: i32 = 507;
const INSTRUMENT: i32 = 508;
// Opt-in diagnostics live only in the separate configuration process. Each
// stage is written before the next native call, so a stall leaves its location.
struct EditorTrace {
    file: std::fs::File,
    started: std::time::Instant,
    remaining: usize,
}
thread_local! {
    static TRACE: RefCell<Option<EditorTrace>> = const { RefCell::new(None) };
}
fn trace(stage: &str) {
    TRACE.with(|slot| {
        if let Some(t) = slot.borrow_mut().as_mut() {
            if t.remaining > 0 {
                t.remaining -= 1;
                let _ = writeln!(
                    t.file,
                    "{:.3}ms {}",
                    t.started.elapsed().as_secs_f64() * 1000.,
                    stage
                );
                if t.remaining == 0 {
                    let _ = writeln!(t.file, "Trace limit reached; recording stopped.");
                }
            }
        }
    });
}
struct Display {
    handle: HMONITOR,
    work: RECT,
}
struct Editor {
    hwnd: HWND,
    root: PathBuf,
    c: Config,
    /// Index into `c.universes` of the tree being edited.
    universe: usize,
    tree_items: Vec<(u64, HTREEITEM)>,
    tree_rows: Vec<editing::TreeRow>,
    tree_universe: Option<u64>,
    tree_views: HashMap<u64, TreeViewState>,
    tree_refresh_pending: bool,
    parents: Vec<u64>,
    selected: u64,
    displays: Vec<Display>,
    monitor: usize,
    json: HWND,
    profiles: Option<Profiles>,
    aux: Option<Aux>,
    tools: Option<HWND>,
    draft_writer: Option<crate::draft::Writer>,
    recovery_draft: Option<crate::draft::Draft>,
    recovery_error: Option<String>,
    status: String,
    preview: RECT,
    dirty: bool,
    scroll_x: i32,
    scroll_y: i32,
    saved: Config,
    disk_revision: Result<storage::Revision, String>,
    recovery: Option<String>,
    history: History,
    settings_before: Option<Config>,
    item_dirty: bool,
    settings_invalid: bool,
    measure_preview: bool,
    preview_cache: Option<PreviewCache>,
    preview_model: Arc<Config>,
    preview_worker:
        Option<crate::background::Latest<crate::preview::Request, Arc<crate::preview::Prepared>>>,
    preview_data: Option<Arc<crate::preview::Prepared>>,
    form: FormLayout,
    controls: Vec<PlacedControl>,
    layout_applied: bool,
    dpi: Dpi,
    item_scroll: i32,
    preview_visible: bool,
    pane_wheel: [i32; 2],
    icons: super::item_icons::Cache,
    icon_draft: ItemIcon,
    advice_worker: Option<crate::background::Latest<Target, String>>,
}
#[derive(Default)]
struct TreeViewState {
    expanded: HashSet<u64>,
    selected: u64,
    first_visible: Option<u64>,
    horizontal: i32,
}
struct PreviewCache {
    prepared: Arc<crate::preview::Prepared>,
    selected: u64,
    universe: usize,
    monitor: usize,
    width: i32,
    height: i32,
    bitmap: HBITMAP,
    sectors: usize,
}
impl Drop for PreviewCache {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.bitmap);
        }
    }
}
#[derive(Clone, Copy)]
struct PlacedControl {
    hwnd: HWND,
    id: i32,
    base: FormRect,
}
thread_local! {
    static RESIZING: Cell<bool> = const { Cell::new(false) };
    static ROOT_WINDOW:Cell<HWND>=const {Cell::new(null_mut())};
    static NEW_CONTROLS:RefCell<Vec<PlacedControl>>=const {RefCell::new(Vec::new())};
}
fn native(r: FormRect) -> RECT {
    RECT {
        left: r.x,
        top: r.y,
        right: r.x + r.w,
        bottom: r.y + r.h,
    }
}
static mut EDITOR: *mut Editor = null_mut();

pub fn spawn() -> Result<(), String> {
    std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg("--configure")
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
unsafe fn notify_resident() {
    let hwnd = FindWindowW(wide("Constellation.Native.Radial").as_ptr(), null());
    if !hwnd.is_null() {
        PostMessageW(hwnd, super::runtime::RELOAD_CONFIG, 0, 0);
    }
}

fn integration_label(id: i32, enabled: bool) -> &'static str {
    match (id == STARTUP, enabled) {
        (true, true) => "Disable Windows startup",
        (true, false) => "Enable Windows startup",
        (false, true) => "Remove from Start menu",
        (false, false) => "Add to Start menu",
    }
}

unsafe fn dlg(hwnd: HWND, id: i32) -> HWND {
    let found = windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem(hwnd, id);
    if !found.is_null() {
        return found;
    }
    for pane in [crate::compact_layout::ITEM, crate::compact_layout::SIDE] {
        let panel = windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem(hwnd, pane);
        if !panel.is_null() {
            let child = windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem(panel, id);
            if !child.is_null() {
                return child;
            }
        }
    }
    null_mut()
}
unsafe fn control(
    hwnd: HWND,
    class: &str,
    label: &str,
    id: i32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    style: u32,
) -> HWND {
    let style = if class == "BUTTON" && matches!(style & BS_TYPEMASK as u32, 0 | 1) {
        (style & !(BS_TYPEMASK as u32)) | BS_OWNERDRAW as u32
    } else {
        style
    };
    let parent = hwnd;
    let dpi = Dpi(GetDpiForWindow(parent));
    let hwnd = CreateWindowExW(
        0,
        wide(class).as_ptr(),
        wide(label).as_ptr(),
        WS_CHILD | WS_VISIBLE | style | if class == "STATIC" { 0 } else { WS_TABSTOP },
        dpi.pixels(x),
        dpi.pixels(y),
        dpi.pixels(w),
        dpi.pixels(h),
        hwnd,
        id as usize as HMENU,
        GetModuleHandleW(null()),
        null(),
    );
    theme::control(hwnd, class);
    if ROOT_WINDOW.with(|root| root.get() == parent) {
        NEW_CONTROLS.with(|items| {
            items.borrow_mut().push(PlacedControl {
                hwnd,
                id,
                base: FormRect { x, y, w, h },
            })
        });
    }
    hwnd
}
unsafe fn browser_header(hwnd: HWND) {
    let r = crate::editor_layout::TARGET_CAPTION;
    label(hwnd, "Target path / URL", r.x, r.y, r.w);
    let r = crate::editor_layout::BROWSER_CONTROL;
    control(
        hwnd,
        "BUTTON",
        "Web browser…",
        BROWSER,
        r.x,
        r.y,
        r.w,
        r.h,
        WS_TABSTOP,
    );
}
unsafe fn get(hwnd: HWND, id: i32) -> String {
    let e = dlg(hwnd, id);
    let len = GetWindowTextLengthW(e);
    let mut b = vec![0u16; len as usize + 1];
    GetWindowTextW(e, b.as_mut_ptr(), b.len() as i32);
    String::from_utf16_lossy(&b[..len as usize])
}
unsafe fn set(hwnd: HWND, id: i32, s: &str) {
    SetWindowTextW(dlg(hwnd, id), wide(s).as_ptr());
}
unsafe fn send(hwnd: HWND, id: i32, msg: u32, w: usize, l: isize) -> isize {
    SendMessageW(dlg(hwnd, id), msg, w, l)
}
unsafe fn label(hwnd: HWND, text: &str, x: i32, y: i32, w: i32) {
    control(hwnd, "STATIC", text, 0, x, y, w, 18, 0);
}

pub fn run(root: PathBuf) -> Result<(), String> {
    if std::env::args().any(|s| s == "--trace-editor") {
        std::fs::create_dir_all(root.join("data")).map_err(|e| e.to_string())?;
        let file = std::fs::File::create(root.join("data/editor-trace.log"))
            .map_err(|e| format!("Cannot create editor trace: {e}"))?;
        TRACE.with(|slot| {
            *slot.borrow_mut() = Some(EditorTrace {
                file,
                started: std::time::Instant::now(),
                remaining: 4096,
            })
        });
        trace(concat!("editor.start version=", env!("CARGO_PKG_VERSION")));
    }
    unsafe {
        // One editor at a time. The tray and a Settings star both spawn this same
        // process, and two editors would each hold their own draft of the whole
        // configuration, so whichever saved last would silently discard the other.
        // Raising the existing window beats refusing: the user asked to edit.
        let existing = FindWindowW(wide("Constellation.Config").as_ptr(), null());
        if !existing.is_null() {
            ShowWindow(existing, SW_SHOWMAXIMIZED);
            SetForegroundWindow(existing);
            return Ok(());
        }
        let controls = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_BAR_CLASSES | ICC_TREEVIEW_CLASSES,
        };
        if InitCommonControlsEx(&controls) == 0 {
            return Err("Cannot initialize editor controls".into());
        }
        if CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) < 0 {
            return Err("Cannot initialize folder picker".into());
        }
        let before_load = storage::revision(&root)?;
        let c = storage::load_or_create(&root)?;
        let loaded_revision = storage::revision(&root)?;
        let recovery = storage::recovery_notice(&root);
        if storage::read(&root).is_ok_and(|disk| disk != c)
            || (before_load != loaded_revision && before_load.exists())
        {
            return Err("Configuration changed while opening the editor; please reopen".into());
        }
        theme::install();
        let module = GetModuleHandleW(null());
        let class = wide("Constellation.Config");
        let wc = WNDCLASSW {
            hIcon: super::icons::large(),
            lpfnWndProc: Some(proc),
            hInstance: module,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            ..zeroed()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_COMPOSITED,
            class.as_ptr(),
            wide(concat!(
                "Constellation ",
                env!("CARGO_PKG_VERSION"),
                " — Configure & Visualize"
            ))
            .as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VSCROLL | WS_HSCROLL | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1380,
            870,
            null_mut(),
            null_mut(),
            module,
            null(),
        );
        super::icons::set_small(hwnd);
        if hwnd.is_null() {
            return Err("Cannot create configuration window".into());
        }
        ROOT_WINDOW.with(|root| root.set(hwnd));
        NEW_CONTROLS.with(|items| items.borrow_mut().clear());
        theme::window(hwnd);
        let mut displays = vec![];
        EnumDisplayMonitors(
            null_mut(),
            null(),
            Some(monitor_callback),
            &mut displays as *mut Vec<Display> as isize,
        );
        EDITOR = Box::into_raw(Box::new(Editor::new(hwnd, root, c, displays)));
        (*EDITOR).disk_revision = Ok(loaded_revision);
        (*EDITOR).recovery = recovery;
        let startup_guard = super::DispatchGuard::enter().expect("editor startup dispatch");
        label(hwnd, "UNIVERSE", 20, 18, 270);
        control(
            hwnd,
            "COMBOBOX",
            "",
            UNIVERSE,
            20,
            40,
            182,
            200,
            WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32,
        );
        control(
            hwnd,
            "BUTTON",
            "+ Universe",
            ADD_UNIVERSE,
            210,
            40,
            70,
            26,
            0,
        );
        label(hwnd, "STARS & CONSTELLATIONS", 20, 74, 270);
        control(
            hwnd,
            "SysTreeView32",
            "",
            TREE,
            20,
            98,
            260,
            417,
            WS_BORDER
                | WS_TABSTOP
                | TVS_HASBUTTONS
                | TVS_HASLINES
                | TVS_LINESATROOT
                | TVS_SHOWSELALWAYS
                | TVS_DISABLEDRAGDROP,
        );
        send(
            hwnd,
            TREE,
            TVM_SETEXTENDEDSTYLE,
            TVS_EX_DOUBLEBUFFER as usize,
            TVS_EX_DOUBLEBUFFER as isize,
        );
        for (id, title, rect) in crate::editor_layout::CREATE_CONTROLS {
            control(hwnd, "BUTTON", title, id, rect.x, rect.y, rect.w, rect.h, 0);
        }
        for (id, title, x, y, w) in [
            (DELETE, "Delete", 20, 565, 80),
            (UP, "Up", 110, 565, 80),
            (DOWN, "Down", 200, 565, 80),
        ] {
            control(hwnd, "BUTTON", title, id, x, y, w, 30, 0);
        }
        // Read-only but selectable: long paths can be scrolled/copied without
        // truncating ancestor names or making the item form taller.
        control(
            hwnd,
            "EDIT",
            "",
            ITEM_PATH,
            305,
            14,
            360,
            25,
            ES_READONLY as u32 | ES_AUTOHSCROLL as u32 | WS_TABSTOP,
        );
        // Name/Kind and Parent/Icon share a row each, the way the layout fields
        // below already do, which is what frees the band for "Visible in".
        for (id, title, x, y, w) in [
            (NAME, "Name", 305, 45, 175),
            (KIND, "Kind", 490, 45, 175),
            (NOTE, "Note", 305, 108, 360),
            (TARGET, "Target path / URL", 305, 171, 360),
            (
                ARGS,
                "Arguments (quote values containing spaces)",
                305,
                234,
                360,
            ),
            (
                RUNNER,
                "Script runner (blank = extension default)",
                305,
                297,
                360,
            ),
            (
                CWD,
                "Working directory (blank = portable folder)",
                305,
                360,
                360,
            ),
            (PARENT, "Parent constellation", 305, 423, 175),
        ] {
            if id == NOTE {
                control(hwnd, "STATIC", "Note (0/140)", NOTE_COUNT, x, y, w, 20, 0);
            } else if id != TARGET {
                label(hwnd, title, x, y, w);
            }
            let combo = matches!(id, KIND | PARENT);
            control(
                hwnd,
                if combo { "COMBOBOX" } else { "EDIT" },
                "",
                id,
                x,
                y + 21,
                if matches!(id, TARGET | RUNNER | CWD) {
                    280
                } else {
                    w
                },
                if combo { 250 } else { 28 },
                WS_BORDER
                    | WS_TABSTOP
                    | if combo {
                        CBS_DROPDOWNLIST as u32 | WS_VSCROLL
                    } else {
                        ES_AUTOHSCROLL as u32
                    },
            );
        }
        browser_header(hwnd);
        send(
            hwnd,
            NOTE,
            EM_SETLIMITTEXT,
            crate::config::NOTE_MAX_CHARS * 2,
            0,
        );
        for title in [
            "Constellation",
            "Program",
            "Windows shortcut / file",
            "URL",
            "Filesystem folder",
            "Script",
        ] {
            send(hwnd, KIND, CB_ADDSTRING, 0, wide(title).as_ptr() as isize);
        }
        for (id, y) in [
            (BROWSE_TARGET, 194),
            (BROWSE_RUNNER, 320),
            (BROWSE_CWD, 383),
        ] {
            control(hwnd, "BUTTON", "Browse…", id, 592, y, 73, 28, 0);
        }
        control(hwnd, "STATIC", "", ADVICE, 465, 585, 200, 48, 0);
        label(
            hwnd,
            "Visible in (none selected = every profile)",
            305,
            484,
            175,
        );
        control(
            hwnd,
            "LISTBOX",
            "",
            VISIBLE,
            305,
            505,
            175,
            72,
            WS_BORDER | WS_VSCROLL | WS_TABSTOP | LBS_NOTIFY as u32 | LBS_MULTIPLESEL as u32,
        );
        control(
            hwnd,
            "COMBOBOX",
            "",
            ICON_MODE,
            490,
            444,
            175,
            180,
            WS_TABSTOP | CBS_DROPDOWNLIST as u32,
        );
        for title in [
            "Icon: Automatic",
            "Icon: Star symbol",
            "Icon: Custom image",
            "Icon: From Software",
        ] {
            send(
                hwnd,
                ICON_MODE,
                CB_ADDSTRING,
                0,
                wide(title).as_ptr() as isize,
            );
        }
        control(hwnd, "BUTTON", "Image…", ICON_BROWSE, 490, 484, 73, 28, 0);
        for (id, rect, enabled) in [
            (
                STARTUP,
                crate::editor_layout::STARTUP_CONTROL,
                super::startup::enabled(),
            ),
            (
                START_MENU,
                crate::editor_layout::START_MENU_CONTROL,
                super::startup::start_menu_enabled(),
            ),
        ] {
            control(
                hwnd,
                "BUTTON",
                integration_label(id, enabled),
                id,
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                WS_TABSTOP,
            );
        }
        control(hwnd, "BUTTON", "Undo", UNDO, 20, 610, 120, 32, 0);
        control(hwnd, "BUTTON", "Redo", REDO, 150, 610, 130, 32, 0);
        control(hwnd, "BUTTON", "Import…", IMPORT, 20, 652, 120, 32, 0);
        control(hwnd, "BUTTON", "Export…", EXPORT, 150, 652, 130, 32, 0);
        control(
            hwnd,
            "STATIC",
            "Drop files or folders here to add Stars. Imports stay in the draft until Save.",
            0,
            20,
            702,
            260,
            56,
            0,
        );
        control(hwnd, "BUTTON", "Update draft", APPLY, 305, 585, 150, 32, 0);
        label(
            hwnd,
            "LAYOUT (numeric values; no forced fitting)",
            305,
            638,
            370,
        );
        for (id, title, x, y) in [
            (RADIUS, "Radius", 305, 668),
            (DISTANCE, "Distance × radius", 490, 668),
            (SPREAD, "Spread °", 305, 728),
            (GAP, "Gap", 490, 728),
        ] {
            label(hwnd, title, x, y, 175);
            control(
                hwnd,
                "EDIT",
                "",
                id,
                x,
                y + 20,
                80,
                28,
                WS_BORDER | ES_AUTOHSCROLL as u32 | WS_TABSTOP,
            );
            control(
                hwnd,
                "msctls_trackbar32",
                "",
                id + 10,
                x + 83,
                y + 18,
                92,
                32,
                WS_TABSTOP | TBS_NOTICKS,
            );
            let (min, max) = match id {
                RADIUS => (32, 400),
                DISTANCE => (0, 200),
                SPREAD => (10, 360),
                _ => (0, 24),
            };
            send(hwnd, id + 10, TBM_SETRANGEMIN, 0, min);
            send(hwnd, id + 10, TBM_SETRANGEMAX, 1, max);
        }
        label(hwnd, "Sound", 305, 800, 90);
        control(
            hwnd,
            "COMBOBOX",
            "",
            INSTRUMENT,
            405,
            795,
            260,
            160,
            WS_TABSTOP | CBS_DROPDOWNLIST as u32,
        );
        for title in [
            "None",
            "Soft mallet",
            "Classic bell",
            "Plucked strings",
            "Wind chimes",
        ] {
            send(
                hwnd,
                INSTRUMENT,
                CB_ADDSTRING,
                0,
                wide(title).as_ptr() as isize,
            );
        }
        label(hwnd, "Volume (0–1)", 430, 838, 110);
        control(
            hwnd,
            "EDIT",
            "",
            VOLUME,
            550,
            833,
            110,
            28,
            WS_BORDER | ES_AUTOHSCROLL as u32,
        );
        label(hwnd, "FULL FAN-OUT PREVIEW", 700, 18, 300);
        control(
            hwnd,
            "COMBOBOX",
            "",
            MONITOR,
            700,
            40,
            320,
            250,
            WS_BORDER | CBS_DROPDOWNLIST as u32 | WS_VSCROLL,
        );
        let e = &mut *EDITOR;
        for (i, d) in e.displays.iter().enumerate() {
            send(
                hwnd,
                MONITOR,
                CB_ADDSTRING,
                0,
                wide(format!(
                    "Monitor {} · {} × {} work area",
                    i + 1,
                    d.work.right - d.work.left,
                    d.work.bottom - d.work.top
                ))
                .as_ptr() as isize,
            );
        }
        send(hwnd, MONITOR, CB_SETCURSEL, 0, 0);
        control(
            hwnd,
            "BUTTON",
            "Save",
            SAVE,
            20,
            906,
            100,
            35,
            BS_DEFPUSHBUTTON as u32,
        );
        control(hwnd, "BUTTON", "Reload", RELOAD, 130, 906, 100, 35, 0);
        control(
            hwnd,
            "BUTTON",
            "Shortcut profiles…",
            PROFILES,
            240,
            906,
            250,
            35,
            0,
        );
        control(
            hwnd,
            "BUTTON",
            "Advanced JSON…",
            ADVANCED,
            500,
            906,
            170,
            35,
            WS_TABSTOP,
        );
        theme::refresh(hwnd, e.c.theme);
        control(
            hwnd, "BUTTON", "Tools...", TOOLS, 690, 906, 150, 35, WS_TABSTOP,
        );
        e.refresh();
        e.settings_to_controls();
        e.dirty = false;
        e.controls = NEW_CONTROLS.with(|items| std::mem::take(&mut *items.borrow_mut()));
        let item_pane = super::editor_pane::create(hwnd, crate::compact_layout::ITEM);
        let side_pane = super::editor_pane::create(hwnd, crate::compact_layout::SIDE);
        SetWindowPos(
            item_pane,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        SetWindowPos(
            side_pane,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        for c in &e.controls {
            if c.base.y < 906 {
                if crate::compact_layout::item(c.base) {
                    SetParent(c.hwnd, item_pane);
                } else if c.base.x < 295 {
                    SetParent(c.hwnd, side_pane);
                }
            }
        }
        control(
            hwnd,
            "BUTTON",
            "Hide preview",
            crate::compact_layout::TOGGLE,
            850,
            906,
            150,
            35,
            WS_TABSTOP,
        );
        e.controls
            .extend(NEW_CONTROLS.with(|items| std::mem::take(&mut *items.borrow_mut())));
        e.resize_view();
        DragAcceptFiles(hwnd, 1);
        drop(startup_guard);
        ShowWindow(hwnd, SW_SHOWMAXIMIZED);
        UpdateWindow(hwnd);
        super::focus_window(hwnd);
        if (*EDITOR).has_recovery() {
            PostMessageW(hwnd, WM_COMMAND, RECOVER_DRAFT as usize, 0);
        }
        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            let e = &*EDITOR;
            let dialog = e.tools.unwrap_or_else(|| {
                e.aux
                    .as_ref()
                    .map(|a| a.hwnd)
                    .or_else(|| e.profiles.as_ref().map(|p| p.hwnd))
                    .unwrap_or(if e.json.is_null() { hwnd } else { e.json })
            });
            let recording = e.profiles.as_ref().is_some_and(|p| p.recording);
            if !recording && editor_accelerator(&msg, hwnd, e) {
                PostMessageW(hwnd, super::editor_pane::REVEAL, 0, GetFocus() as isize);
                continue;
            }
            if recording || IsDialogMessageW(dialog, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if msg.message == WM_KEYDOWN && msg.wParam == 9 {
                PostMessageW(hwnd, super::editor_pane::REVEAL, 0, GetFocus() as isize);
            }
        }
        drop(Box::from_raw(EDITOR));
        EDITOR = null_mut();
        ROOT_WINDOW.with(|root| root.set(null_mut()));
        theme::shutdown();
        CoUninitialize();
        trace("editor.exit");
        TRACE.with(|slot| slot.borrow_mut().take());
        Ok(())
    }
}
unsafe extern "system" fn monitor_callback(
    handle: HMONITOR,
    _: HDC,
    _: *mut RECT,
    l: LPARAM,
) -> i32 {
    let displays = &mut *(l as *mut Vec<Display>);
    let mut info: MONITORINFO = zeroed();
    info.cbSize = size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(handle, &mut info) != 0 {
        displays.push(Display {
            handle,
            work: info.rcWork,
        });
    }
    1
}

impl Editor {
    fn has_recovery(&self) -> bool {
        self.recovery_draft.is_some() || self.recovery_error.is_some()
    }
    fn new(hwnd: HWND, root: PathBuf, c: Config, displays: Vec<Display>) -> Self {
        let recovery = crate::draft::read(&root);
        let recovery_error = recovery.as_ref().err().cloned();
        let recovery_draft = recovery.ok().flatten();
        let selected = c.universes.first().map_or(0, |u| u.id);
        let advice_root = root.clone();
        let window = hwnd as usize;
        let advice_worker = crate::background::Latest::new(
            move |target| editing::target_advice(&advice_root, target).join(" "),
            move || unsafe {
                PostMessageW(window as HWND, ADVICE_READY, 0, 0);
            },
        )
        .ok();
        Self {
            hwnd,
            universe: 0,
            root: root.clone(),
            saved: c.clone(),
            disk_revision: Err("No loaded disk baseline; Reload before saving".into()),
            recovery: None,
            history: History::default(),
            settings_before: None,
            item_dirty: false,
            settings_invalid: false,
            measure_preview: std::env::args().any(|s| s == "--measure-preview"),
            preview_cache: None,
            preview_model: Arc::new(c.clone()),
            preview_worker: crate::background::Latest::new(
                crate::preview::prepare,
                move || unsafe {
                    PostMessageW(window as HWND, PREVIEW_READY, 0, 0);
                },
            )
            .ok(),
            preview_data: None,
            layout_applied: false,
            item_scroll: 0,
            preview_visible: true,
            pane_wheel: [0, 0],
            dpi: Dpi(unsafe { GetDpiForWindow(hwnd) }),
            icons: super::item_icons::Cache::new(hwnd, root.clone()),
            icon_draft: ItemIcon::Automatic,
            advice_worker,
            form: FormLayout::new(1380, 870, 17, 17),
            controls: vec![],
            c,
            tree_items: vec![],
            tree_rows: vec![],
            tree_universe: None,
            tree_views: HashMap::new(),
            tree_refresh_pending: false,
            parents: vec![],
            selected,
            displays,
            monitor: 0,
            json: null_mut(),
            profiles: None,
            aux: None,
            tools: None,
            draft_writer: crate::draft::Writer::new(root.clone()).ok(),
            recovery_draft,
            recovery_error,
            status: "Edit items, tune spacing or shortcut profiles, then Save.".into(),
            preview: RECT {
                left: 700,
                top: 85,
                right: 1335,
                bottom: 690,
            },
            dirty: false,
            scroll_x: 0,
            scroll_y: 0,
        }
    }
    unsafe fn paint_preview(&self, dc: HDC) -> usize {
        let e = self;
        render::fill(dc, &e.preview, theme::background());
        let Some(prepared) = &e.preview_data else {
            return 0;
        };
        let config = &prepared.config;
        let tree = &config.universes[prepared.universe];
        let all = &prepared.sectors;
        let (workw, workh) = (prepared.width, prepared.height);
        let (extentx, extenty) = (prepared.extent_x, prepared.extent_y);
        let r = e.preview;
        let scale = (((r.right - r.left - 30) as f64) / (extentx * 2.))
            .min((r.bottom - r.top - 70) as f64 / (extenty * 2.))
            .min(1.0)
            .max(0.000001);
        let origin = Point {
            x: (r.left + r.right) as f64 / 2.,
            y: (r.top + r.bottom) as f64 / 2.,
        };
        let saved = SaveDC(dc);
        IntersectClipRect(dc, r.left, r.top, r.right, r.bottom);
        let pen = CreatePen(PS_DASH, 1, render::rgb(112, 125, 153));
        let oldp = SelectObject(dc, pen);
        let oldb = SelectObject(dc, GetStockObject(NULL_BRUSH));
        Rectangle(
            dc,
            (origin.x - workw * scale / 2.) as i32,
            (origin.y - workh * scale / 2.) as i32,
            (origin.x + workw * scale / 2.) as i32,
            (origin.y + workh * scale / 2.) as i32,
        );
        SelectObject(dc, oldp);
        SelectObject(dc, oldb);
        DeleteObject(pen);
        let mut nodes = HashMap::new();
        fn index<'a>(n: &'a Node, out: &mut HashMap<u64, &'a Node>) {
            out.insert(n.id, n);
            for child in n.children.iter().flatten() {
                index(child, out);
            }
        }
        index(tree, &mut nodes);
        let images = e.icons.collect(
            nodes
                .get(&e.selected)
                .copied()
                .into_iter()
                .chain(std::iter::once(tree))
                .chain(
                    all.iter()
                        .filter(|s| {
                            (s.inner + s.outer) * 0.5 * s.sweep * scale >= 18.
                                && (s.outer - s.inner) * scale >= 18.
                        })
                        .filter_map(|s| nodes.get(&s.id).copied()),
                ),
        );
        render::draw_with_icons(
            dc,
            config,
            tree,
            all,
            &[],
            None,
            origin,
            scale,
            true,
            &images,
        );
        RestoreDC(dc, saved);
        render::text(
            dc,
            &format!(
                "Overview zoom: {:.0}% · dashed outline = monitor work area",
                scale * 100.
            ),
            r.left,
            r.bottom - 25,
            r.right - r.left,
            render::rgb(148, 164, 190),
            12,
        );
        render::text(
            dc,
            &prepared.warnings,
            r.left,
            r.bottom + 8,
            r.right - r.left,
            theme::muted(),
            12,
        );
        all.len()
    }
    unsafe fn cached_preview(&mut self, dc: HDC) -> usize {
        if self.preview_model.universes.get(self.universe).is_none()
            || !editing::preview_tree_equal(&self.preview_model, &self.c, self.universe)
        {
            self.preview_model = Arc::new(self.c.clone());
        }
        let (width_work, height_work) = if let Some(d) = self.displays.get(self.monitor) {
            let (mut x, mut y) = (96, 96);
            GetDpiForMonitor(d.handle, MDT_EFFECTIVE_DPI, &mut x, &mut y);
            (
                (d.work.right - d.work.left) as f64 * 96. / x.max(1) as f64,
                (d.work.bottom - d.work.top) as f64 * 96. / y.max(1) as f64,
            )
        } else {
            (1920., 1080.)
        };
        let request = crate::preview::Request {
            config: self.preview_model.clone(),
            universe: self.universe,
            layout: self.c.layout,
            theme: self.c.theme,
            width: width_work,
            height: height_work,
        };
        if let Some(data) = self
            .preview_worker
            .as_ref()
            .and_then(|worker| worker.get(&request))
        {
            self.preview_data = Some(data);
        }
        let r = self.preview;
        let width = (r.right - r.left).max(1);
        let height = (r.bottom - r.top + 110).max(1);
        let Some(prepared) = self.preview_data.as_ref() else {
            render::text(
                dc,
                "Preparing preview…",
                r.left,
                r.top + 40,
                width,
                theme::muted(),
                14,
            );
            return 0;
        };
        let valid = self.preview_cache.as_ref().is_some_and(|v| {
            Arc::ptr_eq(&v.prepared, prepared)
                && v.selected == self.selected
                && v.universe == self.universe
                && v.monitor == self.monitor
                && v.width == width
                && v.height == height
        });
        if !valid {
            trace("preview.rebuild.begin");
            let memory = CreateCompatibleDC(dc);
            let bitmap =
                CreateCompatibleBitmap(dc, self.dpi.pixels(width), self.dpi.pixels(height));
            theme::logical_dc(memory, self.dpi.0);
            let old = SelectObject(memory, bitmap);
            render::fill(
                memory,
                &RECT {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                },
                theme::background(),
            );
            SetViewportOrgEx(
                memory,
                -self.dpi.pixels(r.left),
                -self.dpi.pixels(r.top),
                null_mut(),
            );
            let sectors = self.paint_preview(memory);
            SelectObject(memory, old);
            DeleteDC(memory);
            self.preview_cache = Some(PreviewCache {
                prepared: self.preview_data.as_ref().unwrap().clone(),
                selected: self.selected,
                universe: self.universe,
                monitor: self.monitor,
                width,
                height,
                bitmap,
                sectors,
            });
            trace("preview.rebuild.end");
        }
        let cache = self.preview_cache.as_ref().unwrap();
        let memory = CreateCompatibleDC(dc);
        let old = SelectObject(memory, cache.bitmap);
        theme::logical_dc(memory, self.dpi.0);
        BitBlt(dc, r.left, r.top, width, height, memory, 0, 0, SRCCOPY);
        SelectObject(memory, old);
        DeleteDC(memory);
        cache.sectors
    }
    fn checkpoint_settings(&mut self) {
        if let Some(before) = self.settings_before.take() {
            self.history.record(&before, &self.c);
        }
    }
    fn commit(&mut self, c: Config) {
        self.checkpoint_settings();
        self.history.record(&self.c, &c);
        self.c = c;
        self.dirty = self.c != self.saved;
    }
    unsafe fn update_title(&self) {
        SetWindowTextW(
            self.hwnd,
            wide(format!(
                "Constellation {} — Configure & Visualize{}{}",
                env!("CARGO_PKG_VERSION"),
                if self.dirty || self.item_dirty || self.settings_invalid {
                    " · Unsaved"
                } else {
                    " · Saved"
                },
                if self.recovery.is_some() {
                    " [Recovered backup]"
                } else {
                    ""
                }
            ))
            .as_ptr(),
        );
    }
    unsafe fn advice(&self) {
        self.update_title();
        let mut warning = self
            .c
            .launch_target(self.selected)
            .as_ref()
            .map(|t| {
                self.advice_worker
                    .as_ref()
                    .map(|worker| worker.get(t).unwrap_or_else(|| "Checking target...".into()))
                    .unwrap_or_else(|| "Target checks unavailable".into())
            })
            .unwrap_or_default();
        let browser = crate::organize::browser_summary(&self.c, self.universe, self.selected);
        if !browser.is_empty() {
            warning = format!("{browser}\r\n{warning}");
        }
        if self.c.find(self.selected).is_some_and(|n| {
            matches!(n.icon, ItemIcon::Custom { .. } | ItemIcon::Software { .. })
                && self.icons.failed(n)
        }) {
            warning = format!("Icon unavailable; showing symbol. {warning}");
        }
        if let Some(recovery) = &self.recovery {
            warning = format!("{recovery} {warning}");
        }
        if let Some(message) = self.draft_writer.as_ref().and_then(|w| w.error()) {
            warning = format!("Draft recovery unavailable: {message}. {warning}");
        }
        set(self.hwnd, ADVICE, &warning);
    }
    unsafe fn note_counter(&self) {
        let count = get(self.hwnd, NOTE).chars().count();
        set(
            self.hwnd,
            NOTE_COUNT,
            &format!("Note ({count}/{})", crate::config::NOTE_MAX_CHARS),
        );
    }
    unsafe fn edit_note(&self) {
        let input = get(self.hwnd, NOTE);
        let value = editing::note_input(&input);
        if input != value {
            let (mut start, mut end) = (0u32, 0u32);
            send(
                self.hwnd,
                NOTE,
                EM_GETSEL,
                &mut start as *mut u32 as usize,
                &mut end as *mut u32 as isize,
            );
            set(self.hwnd, NOTE, &value);
            let length = value.encode_utf16().count() as u32;
            send(
                self.hwnd,
                NOTE,
                EM_SETSEL,
                start.min(length) as usize,
                end.min(length) as isize,
            );
        }
        self.note_counter();
    }
    unsafe fn enable_fields(&self) {
        EnableWindow(
            dlg(self.hwnd, DUPLICATE),
            (self.selected != self.c.universes[self.universe].id) as i32,
        );
        set(
            self.hwnd,
            ICON_BROWSE,
            if send(self.hwnd, ICON_MODE, CB_GETCURSEL, 0, 0) == 3 {
                "Browse…"
            } else {
                "Image…"
            },
        );
        let kind = send(self.hwnd, KIND, CB_GETCURSEL, 0, 0);
        EnableWindow(
            dlg(self.hwnd, BROWSER),
            (kind == 3 || self.selected == self.c.universes[self.universe].id) as i32,
        );
        for (id, on) in [
            (TARGET, kind != 0),
            (ARGS, matches!(kind, 1 | 5)),
            (RUNNER, kind == 5),
            (CWD, matches!(kind, 1 | 5)),
            (BROWSE_TARGET, matches!(kind, 1 | 2 | 4 | 5)),
            (BROWSE_RUNNER, kind == 5),
            (BROWSE_CWD, matches!(kind, 1 | 5)),
        ] {
            EnableWindow(dlg(self.hwnd, id), on as i32);
        }
    }
    unsafe fn browse(&mut self, id: i32) -> Result<(), String> {
        let kind = send(self.hwnd, KIND, CB_GETCURSEL, 0, 0);
        let pick = if id == BROWSE_CWD || id == BROWSE_TARGET && kind == 4 {
            Pick::Folder
        } else if id == BROWSE_RUNNER {
            Pick::Runner
        } else {
            Pick::Target
        };
        if let Some(path) = pickers::choose(self.hwnd, &self.root, pick)? {
            let field = match id {
                BROWSE_CWD => CWD,
                BROWSE_RUNNER => RUNNER,
                _ => TARGET,
            };
            set(self.hwnd, field, &editing::portable_path(&self.root, &path));
            self.item_dirty = true;
            self.status = "Path selected. Apply or Save to keep it.".into();
        }
        Ok(())
    }
    unsafe fn dropped(&mut self, paths: Vec<PathBuf>) -> Result<(), String> {
        self.apply()?;
        let parent = if self
            .c
            .find(self.selected)
            .is_some_and(|n| n.children.is_some())
        {
            self.selected
        } else {
            let tree = &self.c.universes[self.universe];
            find_parent(tree, self.selected).unwrap_or(tree.id)
        };
        let mut candidate = self.c.clone();
        // Next ID comes from every universe, never just the edited one.
        let mut id = candidate.max_node_id();
        let mut added = vec![];
        for path in paths {
            id = id.checked_add(1).ok_or("Item ID space exhausted")?;
            added.push(editing::star_from_path(&self.root, &path, id)?);
        }
        if added.is_empty() {
            return Ok(());
        }
        let count = added.len();
        candidate.universes[self.universe]
            .find_mut(parent)
            .and_then(|n| n.children.as_mut())
            .ok_or("Choose a parent")?
            .extend(added);
        candidate.validate()?;
        self.commit(candidate);
        self.selected = id;
        self.refresh();
        self.status = format!("Added {count} Stars. Save to persist them.");
        Ok(())
    }
    unsafe fn travel(&mut self, redo: bool) -> Result<(), String> {
        if self.settings_invalid {
            self.settings_to_controls();
            self.status = "Discarded invalid layout fields.".into();
            return Ok(());
        }
        self.checkpoint_settings();
        if self.item_dirty && self.apply().is_err() {
            self.selection_to_controls();
            self.status = "Discarded invalid unapplied item fields.".into();
            return Ok(());
        }
        let c = if redo {
            self.history.redo(&self.c)
        } else {
            self.history.undo(&self.c)
        };
        if let Some(c) = c {
            self.c = c;
            self.dirty = self.c != self.saved;
            self.refresh();
            self.settings_to_controls();
            self.status = if redo { "Redone." } else { "Undone." }.into();
        }
        Ok(())
    }
    unsafe fn save(&mut self) -> Result<(), String> {
        if self.has_recovery() {
            return Err("Resolve the recovery draft first: Tools > Recover draft".into());
        }
        self.apply()?;
        self.controls_to_settings()?;
        self.checkpoint_settings();
        self.disk_revision = Ok(storage::save_checked(
            &self.root,
            &self.c,
            self.disk_revision.as_ref().map_err(Clone::clone)?,
        )?);
        self.recovery = None;
        let recovery_warning = self
            .draft_writer
            .as_ref()
            .and_then(|writer| writer.clear().err());
        KillTimer(self.hwnd, DRAFT_TIMER);
        self.icons.invalidate();
        self.preview_cache = None;
        if let Some(worker) = &self.advice_worker {
            worker.invalidate();
        }
        notify_resident();
        self.saved = self.c.clone();
        self.dirty = false;
        self.advice();
        self.status =
            "Saved. Launcher reloads in the background; an open menu keeps its current settings."
                .into();
        if let Some(message) = recovery_warning {
            self.status
                .push_str(&format!(" Recovery could not be cleared: {message}"));
        }
        Ok(())
    }
    unsafe fn transfer(&mut self, export: bool) -> Result<(), String> {
        self.apply()?;
        self.controls_to_settings()?;
        self.checkpoint_settings();
        let pick = if export { Pick::Export } else { Pick::Import };
        if let Some(path) = pickers::choose(self.hwnd, &self.root, pick)? {
            if export {
                let live = self.root.join("data");
                if path.parent().and_then(|p| p.canonicalize().ok()) == live.canonicalize().ok()
                    && path.file_name().and_then(|s| s.to_str()).is_some_and(|s| {
                        s.eq_ignore_ascii_case("config.json")
                            || s.eq_ignore_ascii_case("config.backup.json")
                    })
                {
                    return Err(
                        "Use Save for the live configuration; choose a different export file."
                            .into(),
                    );
                }
                if path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("json"))
                {
                    editing::export(&path, &self.c)?;
                    self.status = "Exported JSON; custom images remain external references.".into();
                } else {
                    crate::bundle::export(&path, &self.root, &self.c)?;
                    self.status = "Exported portable bundle with custom images. Launch targets are not copied.".into();
                }
            } else {
                let c = crate::bundle::read_import(&path, &self.root)?;
                self.commit(c);
                self.universe = self.universe.min(self.c.universes.len() - 1);
                self.selected = self.c.universes[self.universe].id;
                self.refresh();
                self.settings_to_controls();
                self.status="Imported into the draft. Undo restores the previous tree; Save applies the import.".into();
            }
        }
        Ok(())
    }
    unsafe fn reposition_controls(&self, _resize: bool) {
        let f = &self.form;
        let flags = SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOREDRAW | SWP_NOCOPYBITS;
        for item in [false, true] {
            let pane = dlg(
                self.hwnd,
                if item {
                    crate::compact_layout::ITEM
                } else {
                    crate::compact_layout::SIDE
                },
            );
            let mut r = crate::compact_layout::pane(f, item);
            r.x -= self.scroll_x;
            let p = self.dpi.rect(r);
            SetWindowPos(pane, null_mut(), p.x, p.y, p.w, p.h, flags);
            let position = if item {
                self.item_scroll
            } else {
                self.scroll_y
            };
            let info = SCROLLINFO {
                cbSize: size_of::<SCROLLINFO>() as u32,
                fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
                nMin: 0,
                nMax: crate::compact_layout::content(f, item) - 1,
                nPage: r.h as u32,
                nPos: position,
                nTrackPos: 0,
            };
            SetScrollInfo(pane, SB_VERT, &info, 1);
        }
        let positions: Vec<_> = self
            .controls
            .iter()
            .map(|c| {
                let mut r = f.control(c.base, c.id);
                if c.base.y >= 906 {
                    r = crate::compact_layout::footer(f, c.base);
                } else if crate::compact_layout::item(c.base) {
                    let p = crate::compact_layout::pane(f, true);
                    r.x -= p.x;
                    r.y -= p.y + self.item_scroll;
                } else if c.base.x < 295 {
                    let p = crate::compact_layout::pane(f, false);
                    r.x -= p.x;
                    r.y -= p.y + self.scroll_y;
                } else {
                    r.x -= self.scroll_x;
                }
                (c.hwnd, self.dpi.rect(r))
            })
            .collect();
        let mut batch = BeginDeferWindowPos(positions.len() as i32);
        for &(hwnd, r) in &positions {
            if batch.is_null() {
                break;
            }
            batch = DeferWindowPos(batch, hwnd, null_mut(), r.x, r.y, r.w, r.h, flags);
        }
        if batch.is_null() || EndDeferWindowPos(batch) == 0 {
            for (hwnd, r) in positions {
                SetWindowPos(hwnd, null_mut(), r.x, r.y, r.w, r.h, flags);
            }
        }
        for c in &self.controls {
            if c.base.x >= 690 && c.base.y < 906 {
                ShowWindow(
                    c.hwnd,
                    if self.preview_visible {
                        SW_SHOWNA
                    } else {
                        SW_HIDE
                    },
                );
            }
        }
        RedrawWindow(
            self.hwnd,
            null(),
            null_mut(),
            RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_NOERASE,
        );
    }
    unsafe fn reveal_control(&mut self, focus: HWND) {
        let Some(c) = self.controls.iter().find(|c| c.hwnd == focus) else {
            return;
        };
        if c.base.y >= 906 || c.base.x >= 690 {
            return;
        }
        let item = crate::compact_layout::item(c.base);
        let pane = crate::compact_layout::pane(&self.form, item);
        let r = self.form.control(c.base, c.id);
        let current = if item {
            self.item_scroll
        } else {
            self.scroll_y
        };
        // Combo base heights include dropdowns; reveal the closed focus row.
        let next = crate::compact_layout::reveal(
            current,
            r.y - pane.y,
            r.h.min(32),
            pane.h,
            crate::compact_layout::maximum(&self.form, item),
        );
        if next != current {
            if item {
                self.item_scroll = next;
            } else {
                self.scroll_y = next;
            }
            self.reposition_controls(false);
        }
    }
    unsafe fn scroll_pane(&mut self, item: bool, request: i32, wheel: Option<i32>) {
        let hwnd = dlg(
            self.hwnd,
            if item {
                crate::compact_layout::ITEM
            } else {
                crate::compact_layout::SIDE
            },
        );
        let mut info: SCROLLINFO = zeroed();
        info.cbSize = size_of::<SCROLLINFO>() as u32;
        info.fMask = SIF_ALL;
        GetScrollInfo(hwnd, SB_VERT, &mut info);
        let current = if item {
            self.item_scroll
        } else {
            self.scroll_y
        };
        let proposed = if let Some(delta) = wheel {
            let slot = &mut self.pane_wheel[usize::from(item)];
            *slot += delta;
            let steps = *slot / 120;
            *slot %= 120;
            current - steps * 48
        } else {
            match request {
                SB_LINEUP => current - 24,
                SB_LINEDOWN => current + 24,
                SB_PAGEUP => current - info.nPage as i32,
                SB_PAGEDOWN => current + info.nPage as i32,
                SB_THUMBPOSITION | SB_THUMBTRACK => info.nTrackPos,
                SB_TOP => 0,
                SB_BOTTOM => i32::MAX,
                _ => current,
            }
        };
        let next = proposed.clamp(0, crate::compact_layout::maximum(&self.form, item));
        if next != current {
            if item {
                self.item_scroll = next;
            } else {
                self.scroll_y = next;
            }
            self.reposition_controls(false);
        }
    }
    unsafe fn scroll_to(&mut self, x: i32, y: i32) {
        let (x, _) = self.form.scroll(x, 0);
        let y = y.clamp(0, crate::compact_layout::maximum(&self.form, false));
        if (x, y) == (self.scroll_x, self.scroll_y) {
            return;
        }
        self.scroll_x = x;
        self.scroll_y = y;
        SetScrollPos(self.hwnd, SB_HORZ, x, 1);
        SetScrollPos(dlg(self.hwnd, crate::compact_layout::SIDE), SB_VERT, y, 1);
        self.reposition_controls(false);
    }
    unsafe fn resize_view(&mut self) {
        trace("layout.begin");
        let mut client: RECT = zeroed();
        GetClientRect(self.hwnd, &mut client);
        let dpi = GetDpiForWindow(self.hwnd);
        let barw = GetSystemMetricsForDpi(SM_CXVSCROLL, dpi);
        let barh = GetSystemMetricsForDpi(SM_CYHSCROLL, dpi);
        let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
        // Add back only bars that actually consume client space, then resolve
        // both axes together so crossing a threshold cannot leave a dead bar.
        let fullw = client.right + if style & WS_VSCROLL != 0 { barw } else { 0 };
        let fullh = client.bottom + if style & WS_HSCROLL != 0 { barh } else { 0 };
        let next_dpi = Dpi(dpi);
        let next = crate::compact_layout::form(
            next_dpi.logical(fullw),
            next_dpi.logical(fullh),
            next_dpi.logical(barw),
            next_dpi.logical(barh),
            self.preview_visible,
        );
        if self.layout_applied && self.form == next && self.dpi == next_dpi {
            trace("layout.unchanged");
            return;
        }
        self.form = next;
        self.dpi = next_dpi;
        self.preview_cache = None;
        // Scrollbar visibility changes synchronously send WM_SIZE. Both axes
        // are already resolved above; replaying those intermediate sizes can
        // recursively schedule more layout/scrollbar work on restore.
        RESIZING.with(|active| active.set(true));
        let (x, _) = self.form.scroll(self.scroll_x, 0);
        let y = self
            .scroll_y
            .clamp(0, crate::compact_layout::maximum(&self.form, false));
        self.item_scroll = self
            .item_scroll
            .clamp(0, crate::compact_layout::maximum(&self.form, true));
        self.scroll_x = x;
        self.scroll_y = y;
        for (bar, page, extent, pos) in [
            (SB_HORZ, self.form.page_width, self.form.width, x),
            (SB_VERT, self.form.page_height, self.form.page_height, 0),
        ] {
            let info = SCROLLINFO {
                cbSize: size_of::<SCROLLINFO>() as u32,
                fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
                nMin: 0,
                nMax: extent - 1,
                nPage: page as u32,
                nPos: pos,
                nTrackPos: 0,
            };
            trace(if bar == SB_HORZ {
                "layout.scrollbar.horizontal.begin"
            } else {
                "layout.scrollbar.vertical.begin"
            });
            SetScrollInfo(self.hwnd, bar, &info, 1);
            trace("layout.scrollbar.end");
        }
        let mut preview = self.form.preview();
        preview.h = (self.form.page_height - 310).max(1);
        self.preview = native(preview);
        trace("layout.controls.begin");
        self.reposition_controls(true);
        trace("layout.controls.end");
        self.layout_applied = true;
        RESIZING.with(|active| active.set(false));
        trace("layout.end");
    }
    unsafe fn scroll(&mut self, vertical: bool, request: i32) {
        let bar = if vertical { SB_VERT } else { SB_HORZ };
        let mut info: SCROLLINFO = zeroed();
        info.cbSize = size_of::<SCROLLINFO>() as u32;
        info.fMask = SIF_ALL;
        GetScrollInfo(self.hwnd, bar, &mut info);
        let proposed = match request {
            SB_LINELEFT => info.nPos - 24,
            SB_LINERIGHT => info.nPos + 24,
            SB_PAGELEFT => info.nPos - info.nPage as i32,
            SB_PAGERIGHT => info.nPos + info.nPage as i32,
            SB_THUMBPOSITION | SB_THUMBTRACK => info.nTrackPos,
            SB_LEFT => 0,
            SB_RIGHT => info.nMax,
            _ => return,
        };
        info.fMask = SIF_POS;
        info.nPos = proposed;
        let pos = SetScrollInfo(self.hwnd, bar, &info, 1);
        if vertical {
            self.scroll_to(self.scroll_x, pos);
        } else {
            self.scroll_to(pos, self.scroll_y);
        }
    }

    unsafe fn capture_tree_view(&mut self) {
        let Some(universe) = self.tree_universe else {
            return;
        };
        let tree = dlg(self.hwnd, TREE);
        let first = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_FIRSTVISIBLE as usize, 0);
        let selected = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CARET as usize, 0);
        let view = self.tree_views.entry(universe).or_default();
        view.expanded.clear();
        for &(id, handle) in &self.tree_items {
            let mut item = TVITEMW {
                mask: TVIF_STATE,
                hItem: handle,
                stateMask: TVIS_EXPANDED,
                ..zeroed()
            };
            SendMessageW(tree, TVM_GETITEMW, 0, &mut item as *mut _ as isize);
            if item.state & TVIS_EXPANDED != 0 {
                view.expanded.insert(id);
            }
            if handle == first {
                view.first_visible = Some(id);
            }
            if handle == selected {
                view.selected = id;
            }
        }
        view.horizontal = GetScrollPos(tree, SB_HORZ);
    }
    fn tree_handle(&self, id: u64) -> Option<HTREEITEM> {
        self.tree_items
            .iter()
            .find(|(node, _)| *node == id)
            .map(|(_, handle)| *handle)
    }
    fn tree_id(&self, handle: HTREEITEM) -> Option<u64> {
        self.tree_items
            .iter()
            .find(|(_, item)| *item == handle)
            .map(|(id, _)| *id)
    }
    unsafe fn refresh(&mut self) {
        if theme::source() != self.c.theme {
            theme::refresh(self.hwnd, self.c.theme);
            self.preview_cache = None;
        }
        self.tree_refresh_pending = false;
        self.capture_tree_view();
        self.universe = self.universe.min(self.c.universes.len() - 1);
        let tree = dlg(self.hwnd, TREE);
        SendMessageW(tree, WM_SETREDRAW, 0, 0);
        let rows = editing::tree_rows(&self.c.universes[self.universe]);
        let labels_changed = self.tree_rows != rows;
        let reuse = self.tree_universe == Some(self.c.universes[self.universe].id)
            && editing::same_tree_structure(&self.tree_rows, &rows);
        if reuse {
            for (old, row) in self.tree_rows.iter().zip(&rows) {
                if old.name != row.name {
                    let mut text = wide(format!(
                        "{} {}",
                        if row.branch { "✧" } else { "✦" },
                        row.name
                    ));
                    let item = TVITEMW {
                        mask: TVIF_TEXT,
                        hItem: self.tree_handle(row.id).unwrap(),
                        pszText: text.as_mut_ptr(),
                        ..zeroed()
                    };
                    SendMessageW(tree, TVM_SETITEMW, 0, &item as *const _ as isize);
                }
            }
        } else {
            SendMessageW(tree, TVM_DELETEITEM, 0, TVI_ROOT as isize);
            self.tree_items.clear();
        }
        self.tree_rows = rows;
        if labels_changed {
            self.parents.clear();
            send(self.hwnd, PARENT, CB_RESETCONTENT, 0, 0);
        }
        send(self.hwnd, UNIVERSE, CB_RESETCONTENT, 0, 0);
        for u in &self.c.universes {
            send(
                self.hwnd,
                UNIVERSE,
                CB_ADDSTRING,
                0,
                wide(&u.name).as_ptr() as isize,
            );
        }
        send(self.hwnd, UNIVERSE, CB_SETCURSEL, self.universe, 0);
        unsafe fn walk(
            hwnd: HWND,
            n: &Node,
            parent: HTREEITEM,
            depth: usize,
            items: &mut Vec<(u64, HTREEITEM)>,
            parents: &mut Vec<u64>,
            reuse: bool,
            handles: &HashMap<u64, HTREEITEM>,
        ) {
            let mut text = wide(format!(
                "{} {}",
                if n.children.is_some() { "✧" } else { "✦" },
                n.name
            ));
            let insert = TVINSERTSTRUCTW {
                hParent: parent,
                hInsertAfter: TVI_LAST,
                Anonymous: TVINSERTSTRUCTW_0 {
                    item: TVITEMW {
                        mask: TVIF_TEXT | TVIF_PARAM,
                        pszText: text.as_mut_ptr(),
                        lParam: n.id as isize,
                        ..zeroed()
                    },
                },
            };
            let handle = if reuse {
                handles[&n.id]
            } else {
                let handle = send(hwnd, TREE, TVM_INSERTITEMW, 0, &insert as *const _ as isize);
                items.push((n.id, handle));
                handle
            };
            if let Some(children) = &n.children {
                parents.push(n.id);
                let label = wide(format!("{}✧ {}", "  ".repeat(depth), n.name));
                send(hwnd, PARENT, CB_ADDSTRING, 0, label.as_ptr() as isize);
                for child in children {
                    walk(
                        hwnd,
                        child,
                        handle,
                        depth + 1,
                        items,
                        parents,
                        reuse,
                        handles,
                    );
                }
            }
        }
        let root = &self.c.universes[self.universe];
        let root_id = root.id;
        if labels_changed || !reuse {
            let handles = self.tree_items.iter().copied().collect();
            walk(
                self.hwnd,
                root,
                TVI_ROOT,
                0,
                &mut self.tree_items,
                &mut self.parents,
                reuse,
                &handles,
            );
        }
        if root.find(self.selected).is_none() {
            self.selected = root_id;
        }
        let view = self
            .tree_views
            .entry(root_id)
            .or_insert_with(|| TreeViewState {
                expanded: HashSet::from([root_id]),
                selected: root_id,
                ..Default::default()
            });
        // Adding/moving/undoing a selected item reveals its ancestors only.
        let path = editing::node_path(root, self.selected);
        for node in path.iter().take(path.len().saturating_sub(1)) {
            view.expanded.insert(node.id);
        }
        for &(id, handle) in &self.tree_items {
            if view.expanded.contains(&id) {
                SendMessageW(tree, TVM_EXPAND, TVE_EXPAND as usize, handle);
            }
        }
        let first = (view.selected == self.selected)
            .then_some(view.first_visible)
            .flatten();
        let horizontal = view.horizontal;
        if let Some(handle) = self.tree_handle(self.selected) {
            SendMessageW(tree, TVM_SELECTITEM, TVGN_CARET as usize, handle);
        }
        if let Some(handle) = first.and_then(|id| self.tree_handle(id)) {
            SendMessageW(tree, TVM_SELECTITEM, TVGN_FIRSTVISIBLE as usize, handle);
        }
        // TVM_SELECTITEM can scroll horizontally to the label; restore the view.
        SendMessageW(
            tree,
            WM_HSCROLL,
            SB_THUMBPOSITION as usize | ((horizontal as usize) << 16),
            0,
        );
        self.tree_universe = Some(root_id);
        SendMessageW(tree, WM_SETREDRAW, 1, 0);
        InvalidateRect(tree, null(), 0);
        self.selection_to_controls();
        InvalidateRect(self.hwnd, null(), 0);
    }
    /// Profiles that open the universe being edited, as (id, name).
    fn audience(&self) -> Vec<(u64, String)> {
        let universe = self.c.universes[self.universe].id;
        self.c
            .profiles
            .iter()
            .filter(|p| self.c.universe_of(p).is_some_and(|u| u.id == universe))
            .map(|p| (p.id, p.name.clone()))
            .collect()
    }
    unsafe fn selection_to_controls(&mut self) {
        let Some(n) = self.c.find(self.selected) else {
            return;
        };
        let path = editing::node_path(&self.c.universes[self.universe], self.selected)
            .iter()
            .map(|node| node.name.as_str())
            .collect::<Vec<_>>()
            .join(" › ");
        set(self.hwnd, ITEM_PATH, &path);
        send(self.hwnd, ITEM_PATH, EM_SETSEL, 0, 0);
        send(self.hwnd, ITEM_PATH, EM_SCROLLCARET, 0, 0);
        set(self.hwnd, NAME, &n.name);
        set(self.hwnd, NOTE, &n.note);
        self.note_counter();
        self.icon_draft = n.icon.clone();
        let mode = match n.icon {
            ItemIcon::Automatic => 0,
            ItemIcon::Symbol => 1,
            ItemIcon::Custom { .. } => 2,
            ItemIcon::Software { .. } => 3,
        };
        send(self.hwnd, ICON_MODE, CB_SETCURSEL, mode, 0);
        let (mut kind, mut path, mut args, mut runner, mut cwd) =
            (0, String::new(), vec![], String::new(), String::new());
        if let Some(t) = &n.target {
            match t {
                Target::Program {
                    path: p,
                    args: a,
                    cwd: c,
                } => {
                    kind = 1;
                    path = p.clone();
                    args = a.clone();
                    cwd = c.clone().unwrap_or_default();
                }
                Target::Shortcut { path: p } => {
                    kind = 2;
                    path = p.clone();
                }
                Target::Url { url } => {
                    kind = 3;
                    path = url.clone();
                }
                Target::Folder { path: p } => {
                    kind = 4;
                    path = p.clone();
                }
                Target::Script {
                    path: p,
                    args: a,
                    runner: r,
                    cwd: c,
                } => {
                    kind = 5;
                    path = p.clone();
                    args = a.clone();
                    runner = r.clone().unwrap_or_default();
                    cwd = c.clone().unwrap_or_default();
                }
            }
        }
        send(self.hwnd, KIND, CB_SETCURSEL, kind, 0);
        set(self.hwnd, TARGET, &path);
        set(self.hwnd, ARGS, &editing::format_arguments(&args));
        set(self.hwnd, RUNNER, &runner);
        set(self.hwnd, CWD, &cwd);
        let parent = find_parent(&self.c.universes[self.universe], self.selected)
            .unwrap_or(self.c.universes[self.universe].id);
        let idx = self
            .parents
            .iter()
            .position(|id| *id == parent)
            .unwrap_or(0);
        send(self.hwnd, PARENT, CB_SETCURSEL, idx, 0);
        let restricted = n.visible_in.clone();
        send(self.hwnd, VISIBLE, LB_RESETCONTENT, 0, 0);
        for (i, p) in self.audience().into_iter().enumerate() {
            send(
                self.hwnd,
                VISIBLE,
                LB_ADDSTRING,
                0,
                wide(&p.1).as_ptr() as isize,
            );
            if restricted.contains(&p.0) {
                send(self.hwnd, VISIBLE, LB_SETSEL, 1, i as isize);
            }
        }
        self.item_dirty = false;
        self.enable_fields();
        self.advice();
    }
    unsafe fn sync_sliders(&self) {
        for (id, value) in [
            (RADIUS, self.c.layout.radius),
            (DISTANCE, self.c.layout.distance * 100.),
            (SPREAD, self.c.layout.spread),
            (GAP, self.c.layout.gap),
        ] {
            let position = value.round().clamp(0., i32::MAX as f64) as isize;
            if send(self.hwnd, id + 10, TBM_GETPOS, 0, 0) != position {
                send(self.hwnd, id + 10, TBM_SETPOS, 1, position);
            }
        }
    }
    unsafe fn settings_to_controls(&mut self) {
        self.settings_invalid = false;
        self.sync_sliders();
        send(
            self.hwnd,
            INSTRUMENT,
            CB_SETCURSEL,
            match self.c.sound.selected_instrument() {
                None => 0,
                Some(Instrument::Mallet) => 1,
                Some(Instrument::ClassicBell) => 2,
                Some(Instrument::PluckedStrings) => 3,
                Some(Instrument::WindChimes) => 4,
            },
            0,
        );
        let l = self.c.layout;
        for (id, value) in [
            (RADIUS, l.radius),
            (DISTANCE, l.distance),
            (SPREAD, l.spread),
            (GAP, l.gap),
            (VOLUME, self.c.sound.volume),
        ] {
            set(self.hwnd, id, &value.to_string());
        }
    }
    unsafe fn controls_to_settings(&mut self) -> Result<(), String> {
        let number = |id| -> Result<f64, String> {
            get(self.hwnd, id)
                .trim()
                .parse()
                .map_err(|_| "Enter numeric layout and volume values".into())
        };
        let layout = crate::config::Layout {
            radius: number(RADIUS)?,
            distance: number(DISTANCE)?,
            spread: number(SPREAD)?,
            gap: number(GAP)?,
        };
        let mut sound = self.c.sound;
        sound.volume = number(VOLUME)?;
        let instrument = match send(self.hwnd, INSTRUMENT, CB_GETCURSEL, 0, 0) {
            0 => None,
            1 => Some(Instrument::Mallet),
            2 => Some(Instrument::ClassicBell),
            3 => Some(Instrument::PluckedStrings),
            4 => Some(Instrument::WindChimes),
            _ => return Err("Choose a sound".into()),
        };
        sound.select_instrument(instrument);
        crate::config::validate_settings(layout, sound)?;
        if layout != self.c.layout || sound != self.c.sound {
            if self.settings_before.is_none() {
                self.settings_before = Some(self.c.clone());
            }
            self.c.layout = layout;
            self.c.sound = sound;
            self.dirty = self.c != self.saved;
        }
        self.settings_invalid = false;
        Ok(())
    }
    unsafe fn apply(&mut self) -> Result<(), String> {
        self.apply_item(true)
    }
    unsafe fn apply_item(&mut self, refresh: bool) -> Result<(), String> {
        if !self.item_dirty {
            return Ok(());
        }
        let mut candidate = self.c.clone();
        let old = candidate
            .find(self.selected)
            .ok_or("Select an item")?
            .clone();
        let kind = send(self.hwnd, KIND, CB_GETCURSEL, 0, 0);
        let path = get(self.hwnd, TARGET);
        let args = if matches!(kind, 1 | 5) {
            editing::parse_arguments(&get(self.hwnd, ARGS))?
        } else {
            vec![]
        };
        let option = |s: String| if s.trim().is_empty() { None } else { Some(s) };
        let cwd = option(get(self.hwnd, CWD));
        let runner = option(get(self.hwnd, RUNNER));
        let target = match kind {
            0 => None,
            1 => Some(Target::Program { path, args, cwd }),
            2 => Some(Target::Shortcut { path }),
            3 => Some(Target::Url { url: path }),
            4 => Some(Target::Folder { path }),
            5 => Some(Target::Script {
                path,
                runner,
                args,
                cwd,
            }),
            _ => return Err("Choose a kind".into()),
        };
        if target.is_some() && old.children.as_ref().is_some_and(|cs| !cs.is_empty()) {
            return Err(
                "Move/remove this constellation's children before converting it to a Star".into(),
            );
        }
        let audience = self.audience();
        // Keep any restriction aimed at a profile this list does not offer, so a
        // value set by hand in the JSON is not silently dropped here.
        let mut visible_in: Vec<u64> = old
            .visible_in
            .iter()
            .copied()
            .filter(|id| !audience.iter().any(|(p, _)| p == id))
            .collect();
        visible_in.extend(
            audience
                .iter()
                .enumerate()
                .filter(|(i, _)| send(self.hwnd, VISIBLE, LB_GETSEL, *i, 0) > 0)
                .map(|(_, (id, _))| *id),
        );
        let n = candidate.universes[self.universe]
            .find_mut(self.selected)
            .unwrap();
        n.visible_in = visible_in;
        n.name = get(self.hwnd, NAME);
        n.note = get(self.hwnd, NOTE);
        n.children = if target.is_none() {
            Some(old.children.clone().unwrap_or_default())
        } else {
            None
        };
        n.target = target;
        if !matches!(n.target, Some(Target::Url { .. }))
            && n.id != self.c.universes[self.universe].id
        {
            n.browser = None;
        }
        n.icon = match send(self.hwnd, ICON_MODE, CB_GETCURSEL, 0, 0) {
            0 => ItemIcon::Automatic,
            1 => ItemIcon::Symbol,
            2 => match &self.icon_draft {
                ItemIcon::Custom { .. } => self.icon_draft.clone(),
                _ => return Err("Choose an image with Image… first.".into()),
            },
            3 => match &self.icon_draft {
                ItemIcon::Software { .. } => self.icon_draft.clone(),
                _ => return Err("Choose software with Browse… first.".into()),
            },
            _ => return Err("Choose an icon mode.".into()),
        };
        if self.selected != candidate.universes[self.universe].id {
            let pindex = send(self.hwnd, PARENT, CB_GETCURSEL, 0, 0);
            let parent = *self.parents.get(pindex as usize).ok_or("Choose a parent")?;
            if candidate
                .find(self.selected)
                .unwrap()
                .find(parent)
                .is_some()
            {
                return Err("A constellation cannot be moved into itself or a descendant".into());
            }
            if find_parent(&candidate.universes[self.universe], self.selected) != Some(parent) {
                let node = remove(&mut candidate.universes[self.universe], self.selected)
                    .ok_or("Cannot move this item")?;
                candidate.universes[self.universe]
                    .find_mut(parent)
                    .and_then(|p| p.children.as_mut())
                    .ok_or("Parent is not a constellation")?
                    .push(node);
            }
        }
        candidate.validate()?;
        self.commit(candidate);
        self.item_dirty = false;
        if refresh {
            self.refresh();
        }
        Ok(())
    }
    unsafe fn add(&mut self, folder: bool) -> Result<(), String> {
        self.apply()?;
        let before = self.c.clone();
        let parent = if self
            .c
            .find(self.selected)
            .is_some_and(|n| n.children.is_some())
        {
            self.selected
        } else {
            find_parent(&self.c.universes[self.universe], self.selected)
                .unwrap_or(self.c.universes[self.universe].id)
        };
        let id = self
            .c
            .max_node_id()
            .checked_add(1)
            .ok_or("Item ID space exhausted")?;
        let n = Node {
            browser: None,
            note: String::new(),
            icon: Default::default(),
            visible_in: vec![],
            id,
            name: if folder {
                "New constellation"
            } else {
                "New Star"
            }
            .into(),
            children: if folder { Some(vec![]) } else { None },
            target: if folder {
                None
            } else {
                Some(Target::Program {
                    path: "calc.exe".into(),
                    args: vec![],
                    cwd: None,
                })
            },
        };
        self.c.universes[self.universe]
            .find_mut(parent)
            .unwrap()
            .children
            .as_mut()
            .unwrap()
            .push(n);
        self.history.record(&before, &self.c);
        self.selected = id;
        self.dirty = self.c != self.saved;
        self.refresh();
        Ok(())
    }
    unsafe fn duplicate(&mut self) -> Result<(), String> {
        self.apply()?;
        let mut candidate = self.c.clone();
        let id = editing::duplicate_node(&mut candidate, self.universe, self.selected)?;
        self.commit(candidate);
        self.selected = id;
        self.refresh();
        self.status = "Duplicated independently. Edit the copy, then Save.".into();
        SetFocus(dlg(self.hwnd, NAME));
        send(self.hwnd, NAME, EM_SETSEL, 0, -1);
        Ok(())
    }
    unsafe fn reorder(&mut self, down: bool) -> Result<(), String> {
        self.apply()?;
        let before = self.c.clone();
        if let Some(parent) = find_parent(&self.c.universes[self.universe], self.selected) {
            let cs = self.c.universes[self.universe]
                .find_mut(parent)
                .unwrap()
                .children
                .as_mut()
                .unwrap();
            let i = cs.iter().position(|n| n.id == self.selected).unwrap();
            let j = if down {
                (i + 1).min(cs.len() - 1)
            } else {
                i.saturating_sub(1)
            };
            cs.swap(i, j);
            self.history.record(&before, &self.c);
            self.dirty = self.c != self.saved;
            self.refresh();
        }
        Ok(())
    }
}
fn find_parent(root: &Node, id: u64) -> Option<u64> {
    let cs = root.children.as_ref()?;
    if cs.iter().any(|c| c.id == id) {
        return Some(root.id);
    }
    cs.iter().find_map(|c| find_parent(c, id))
}
fn remove(root: &mut Node, id: u64) -> Option<Node> {
    let cs = root.children.as_mut()?;
    if let Some(i) = cs.iter().position(|n| n.id == id) {
        return Some(cs.remove(i));
    }
    cs.iter_mut().find_map(|c| remove(c, id))
}

unsafe fn advanced(e: &mut Editor) {
    if !e.json.is_null() {
        ShowWindow(e.json, SW_SHOW);
        SetForegroundWindow(e.json);
        return;
    }
    let class = wide("Constellation.Advanced");
    let module = GetModuleHandleW(null());
    let wc = WNDCLASSW {
        hIcon: super::icons::large(),
        lpfnWndProc: Some(json_proc),
        hInstance: module,
        lpszClassName: class.as_ptr(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: null_mut(),
        ..zeroed()
    };
    RegisterClassW(&wc);
    let hwnd = CreateWindowExW(
        WS_EX_COMPOSITED,
        class.as_ptr(),
        wide("Constellation — Advanced JSON").as_ptr(),
        WS_OVERLAPPEDWINDOW,
        80,
        80,
        850,
        800,
        e.hwnd,
        null_mut(),
        module,
        null(),
    );
    super::icons::set_small(hwnd);
    theme::window(hwnd);
    e.json = hwnd;
    EnableWindow(e.hwnd, 0);
    label(
        hwnd,
        "Edit the complete configuration. Save validates and writes only inside the portable folder.",
        15,
        12,
        800,
    );
    control(
        hwnd,
        "EDIT",
        &serde_json::to_string_pretty(&e.c)
            .unwrap()
            .replace('\n', "\r\n"),
        JSON_TEXT,
        15,
        40,
        800,
        650,
        WS_BORDER
            | WS_VSCROLL
            | WS_HSCROLL
            | ES_MULTILINE as u32
            | ES_WANTRETURN as u32
            | ES_AUTOHSCROLL as u32
            | ES_AUTOVSCROLL as u32,
    );
    control(
        hwnd,
        "BUTTON",
        "Validate & Save",
        JSON_SAVE,
        15,
        710,
        170,
        32,
        0,
    );
    ShowWindow(hwnd, SW_SHOW);
}

unsafe extern "system" fn json_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(result) = theme::display_message(hwnd, msg, w, l) {
        return result;
    }
    if msg == WM_COMMAND && !EDITOR.is_null() {
        SetTimer((*EDITOR).hwnd, DRAFT_TIMER, 500, None);
    }
    if let Some(result) = theme::message(msg, w, l) {
        return result;
    }
    if EDITOR.is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let Some(_guard) = super::DispatchGuard::enter() else {
        if super::defer_nested_close(hwnd, msg, w, l) {
            return 0;
        }
        if msg == WM_COMMAND && w & 0xffff == JSON_SAVE as usize {
            super::defer_message(hwnd, msg, w, l);
            return 0;
        }
        return DefWindowProcW(hwnd, msg, w, l);
    };
    let e = &mut *EDITOR;
    match msg {
        WM_PAINT => {
            render::buffered(hwnd, theme::background(), |_| {});
            0
        }
        WM_SIZE => {
            let dpi = Dpi(GetDpiForWindow(hwnd));
            let width = (l as u32 & 0xffff) as i32;
            let height = ((l as u32 >> 16) & 0xffff) as i32;
            MoveWindow(
                dlg(hwnd, JSON_TEXT),
                dpi.pixels(15),
                dpi.pixels(40),
                (width - dpi.pixels(30)).max(1),
                (height - dpi.pixels(105)).max(1),
                1,
            );
            MoveWindow(
                dlg(hwnd, JSON_SAVE),
                dpi.pixels(15),
                height - dpi.pixels(50),
                dpi.pixels(170),
                dpi.pixels(32),
                1,
            );
            0
        }
        WM_COMMAND if w & 0xffff == JSON_SAVE as usize => {
            let result = (|| -> Result<(), String> {
                if e.has_recovery() {
                    return Err("Resolve the recovery draft first: Tools > Recover draft".into());
                }
                let c = storage::parse_config(get(hwnd, JSON_TEXT).as_bytes())?;
                e.disk_revision = Ok(storage::save_checked(
                    &e.root,
                    &c,
                    e.disk_revision.as_ref().map_err(Clone::clone)?,
                )?);
                e.recovery = None;
                e.icons.invalidate();
                e.preview_cache = None;
                if let Some(worker) = &e.advice_worker {
                    worker.invalidate();
                }
                notify_resident();
                e.commit(c.clone());
                e.saved = c;
                e.dirty = false;
                if let Some(writer) = &e.draft_writer {
                    let _ = writer.clear();
                }
                e.refresh();
                e.settings_to_controls();
                e.status =
                    "Saved. Launcher reloads in the background; an open menu keeps its current settings.".into();
                InvalidateRect(e.hwnd, null(), 1);
                Ok(())
            })();
            if let Err(err) = result {
                error(&err);
            }
            0
        }
        WM_CLOSE => {
            EnableWindow(e.hwnd, 1);
            DestroyWindow(hwnd);
            e.json = null_mut();
            super::focus_window(e.hwnd);
            0
        }
        _ => {
            // Default non-client handling can run Windows' modal move/size
            // loop. No application-state borrow may span that native loop:
            // its nested events must be able to enter our normal handlers.
            drop(_guard);
            DefWindowProcW(hwnd, msg, w, l)
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_COMMAND && (w & 0xffff) == TOOLS as usize {
        return show_tools_menu(hwnd);
    }
    match msg {
        WM_ENTERSIZEMOVE => trace("native.move_or_resize.begin"),
        WM_EXITSIZEMOVE => trace("native.move_or_resize.end"),
        WM_CAPTURECHANGED => trace("mouse.capture.changed"),
        WM_CANCELMODE => trace("native.cancel_mode"),
        _ => {}
    }
    if let Some(result) = theme::message(msg, w, l) {
        return result;
    }
    if let Some(result) = theme::display_message(hwnd, msg, w, l) {
        return result;
    }
    if msg == WM_DESTROY {
        PostQuitMessage(0);
        return 0;
    }
    if EDITOR.is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let Some(_guard) = super::DispatchGuard::enter() else {
        if super::defer_nested_close(hwnd, msg, w, l) {
            return 0;
        }
        if msg == WM_SIZE {
            trace(if RESIZING.with(Cell::get) {
                "resize.nested.ignored"
            } else {
                "resize.nested.deferred"
            });
        }
        if matches!(
            msg,
            WM_HSCROLL
                | WM_VSCROLL
                | WM_DROPFILES
                | ADVICE_READY
                | PREVIEW_READY
                | super::editor_pane::REVEAL
                | super::item_icons::READY
        ) || (msg == WM_SIZE && !RESIZING.with(Cell::get) && w != SIZE_MINIMIZED as usize)
        {
            super::defer_message(hwnd, msg, w, l);
            return 0;
        }
        if msg == WM_COMMAND
            && (w >> 16) as u32 == BN_CLICKED
            && (w & 0xffff) >= BROWSE_TARGET as usize
        {
            super::defer_message(hwnd, msg, w, l);
            return 0;
        }
        return DefWindowProcW(hwnd, msg, w, l);
    };
    let e = &mut *EDITOR;
    if matches!(msg, WM_COMMAND | WM_HSCROLL) {
        SetTimer(hwnd, DRAFT_TIMER, 500, None);
    }
    match msg {
        WM_TIMER if w == DRAFT_TIMER => {
            KillTimer(hwnd, DRAFT_TIMER);
            e.capture_draft();
            0
        }
        super::editor_tools::APPLY => {
            e.finish_tool(w as HWND);
            0
        }
        super::editor_tools::CLOSED => {
            if e.tools == Some(w as HWND) {
                e.tools = None;
                EnableWindow(hwnd, 1);
                super::focus_window(hwnd);
                SetTimer(hwnd, DRAFT_TIMER, 500, None);
            }
            0
        }
        theme::DISPLAY_CHANGED => {
            theme::apply_display(hwnd, e.c.theme, false);
            e.preview_cache = None;
            e.layout_applied = false;
            e.resize_view();
            0
        }
        super::editor_pane::REVEAL => {
            e.reveal_control(l as HWND);
            0
        }
        super::editor_pane::SCROLL => {
            e.scroll_pane(
                w == crate::compact_layout::ITEM as usize,
                (l as usize & 0xffff) as i32,
                None,
            );
            0
        }
        super::editor_pane::WHEEL => {
            e.scroll_pane(
                w == crate::compact_layout::ITEM as usize,
                0,
                Some((((l as usize) >> 16) as u16 as i16) as i32),
            );
            0
        }
        PREVIEW_READY => {
            InvalidateRect(hwnd, null(), 0);
            0
        }
        REFRESH_TREE => {
            if e.tree_refresh_pending {
                // A newly typed edit must survive even if it arrives before
                // this posted refresh. Invalid fields keep their current form.
                if let Err(message) = e.apply_item(false) {
                    e.status = message;
                    set(hwnd, ADVICE, &e.status);
                    return 0;
                }
                e.refresh();
            }
            0
        }
        WM_NOTIFY if l != 0 && (*(l as *const NMHDR)).idFrom == TREE as usize => {
            let header = &*(l as *const NMHDR);
            match header.code {
                TVN_ITEMEXPANDINGW | TVN_ITEMEXPANDEDW => {
                    let change = &*(l as *const NMTREEVIEWW);
                    if change.action == TVE_COLLAPSE {
                        if let Some(id) = e.tree_id(change.itemNew.hItem) {
                            let hides_selection = id != e.selected
                                && e.c.find(id).is_some_and(|n| n.find(e.selected).is_some());
                            if hides_selection {
                                if header.code == TVN_ITEMEXPANDINGW {
                                    let dirty = e.item_dirty;
                                    if let Err(message) = e.apply_item(false) {
                                        e.status = message;
                                        set(hwnd, ADVICE, &e.status);
                                        return 1;
                                    }
                                    e.tree_refresh_pending |= dirty;
                                } else {
                                    // Native collapse may retain the hidden child's caret.
                                    // Keep the item panel and visible selection together.
                                    e.selected = id;
                                    send(
                                        hwnd,
                                        TREE,
                                        TVM_SELECTITEM,
                                        TVGN_CARET as usize,
                                        change.itemNew.hItem,
                                    );
                                    e.selection_to_controls();
                                    if e.tree_refresh_pending {
                                        PostMessageW(hwnd, REFRESH_TREE, 0, 0);
                                    }
                                }
                            }
                        }
                    }
                    0
                }
                TVN_SELCHANGINGW => {
                    // Never rebuild native items while TreeView owns handles
                    // on its selection stack. Validate now; refresh after it returns.
                    e.checkpoint_settings();
                    let dirty = e.item_dirty;
                    if let Err(message) = e.apply_item(false) {
                        e.status = message;
                        set(hwnd, ADVICE, &e.status);
                        return 1;
                    }
                    if dirty {
                        e.tree_refresh_pending = true;
                    }
                    0
                }
                TVN_SELCHANGEDW => {
                    let change = &*(l as *const NMTREEVIEWW);
                    if let Some(id) = e.tree_id(change.itemNew.hItem) {
                        e.selected = id;
                        e.selection_to_controls();
                    }
                    if e.tree_refresh_pending {
                        PostMessageW(hwnd, REFRESH_TREE, 0, 0);
                    }
                    0
                }
                _ => 0,
            }
        }
        ADVICE_READY => {
            e.advice();
            0
        }
        super::item_icons::READY => {
            e.icons.acknowledge();
            e.preview_cache = None;
            e.advice();
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_DROPFILES => {
            let drop = w as _;
            let count = DragQueryFileW(drop, u32::MAX, null_mut(), 0);
            let mut paths = vec![];
            for i in 0..count {
                let len = DragQueryFileW(drop, i, null_mut(), 0);
                let mut buffer = vec![0u16; len as usize + 1];
                DragQueryFileW(drop, i, buffer.as_mut_ptr(), buffer.len() as u32);
                paths.push(PathBuf::from(String::from_utf16_lossy(
                    &buffer[..len as usize],
                )));
            }
            DragFinish(drop);
            if let Err(message) = e.dropped(paths) {
                e.status = message;
            }
            InvalidateRect(hwnd, null(), 1);
            0
        }
        WM_HSCROLL | WM_VSCROLL if l == 0 => {
            e.scroll(msg == WM_VSCROLL, (w & 0xffff) as i32);
            0
        }
        WM_MOUSEWHEEL => {
            let mut point = POINT {
                x: (l as u16 as i16) as i32,
                y: ((l as usize >> 16) as u16 as i16) as i32,
            };
            ScreenToClient(hwnd, &mut point);
            if let Some(item) = crate::compact_layout::hit_pane(
                &e.form,
                e.scroll_x,
                e.dpi.logical(point.x),
                e.dpi.logical(point.y),
            ) {
                e.scroll_pane(item, 0, Some(((w >> 16) as u16 as i16) as i32));
            }
            0
        }
        WM_HSCROLL if l != 0 => {
            let slider = GetDlgCtrlID(l as HWND);
            let id = slider - 10;
            if matches!(id, RADIUS | DISTANCE | SPREAD | GAP) {
                let value = send(hwnd, slider, WM_USER, 0, 0) as f64
                    / if id == DISTANCE { 100. } else { 1. };
                set(hwnd, id, &value.to_string());
                if e.controls_to_settings().is_ok() {
                    e.dirty = true;
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            0
        }
        WM_COMMAND => {
            let id = (w & 0xffff) as i32;
            let notify = (w >> 16) as u32;
            if matches!(id, NAME | NOTE | TARGET | ARGS | RUNNER | CWD) && notify == EN_CHANGE {
                if id == NOTE {
                    e.edit_note();
                }
                e.item_dirty = true;
                InvalidateRect(hwnd, null(), 1);
                return 0;
            }
            if id == UNIVERSE && notify == CBN_SELCHANGE as u32 {
                let index = send(hwnd, UNIVERSE, CB_GETCURSEL, 0, 0);
                if index >= 0 && (index as usize) < e.c.universes.len() {
                    match e.apply() {
                        Ok(()) => {
                            e.universe = index as usize;
                            let root = e.c.universes[e.universe].id;
                            e.selected = e.tree_views.get(&root).map_or(root, |v| v.selected);
                            e.refresh();
                        }
                        Err(message) => {
                            // Keep the edit and stay put rather than lose it.
                            e.status = message;
                            send(hwnd, UNIVERSE, CB_SETCURSEL, e.universe, 0);
                        }
                    }
                }
                InvalidateRect(hwnd, null(), 1);
                return 0;
            }
            if id == VISIBLE && notify == LBN_SELCHANGE as u32 {
                e.item_dirty = true;
                InvalidateRect(hwnd, null(), 1);
                return 0;
            }
            if matches!(id, KIND | PARENT | ICON_MODE) && notify == CBN_SELCHANGE as u32 {
                e.item_dirty = true;
                e.enable_fields();
                return 0;
            }
            if id == INSTRUMENT && notify == CBN_SELCHANGE as u32 {
                e.checkpoint_settings();
                let result = e.controls_to_settings();
                e.settings_invalid = result.is_err();
                if let Err(message) = result {
                    e.status = message;
                }
                e.checkpoint_settings();
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            if id == MONITOR && notify == CBN_SELCHANGE as u32 {
                e.monitor = send(hwnd, MONITOR, CB_GETCURSEL, 0, 0) as usize;
                InvalidateRect(hwnd, null(), 1);
                return 0;
            }
            if matches!(id, RADIUS | DISTANCE | SPREAD | GAP | VOLUME) && notify == EN_CHANGE {
                let result = e.controls_to_settings();
                e.settings_invalid = result.is_err();
                if result.is_ok() {
                    e.dirty = true;
                    e.sync_sliders();
                    InvalidateRect(hwnd, null(), 1);
                }
                return 0;
            }
            if matches!(id, RADIUS | DISTANCE | SPREAD | GAP | VOLUME) && notify == EN_KILLFOCUS {
                e.checkpoint_settings();
                return 0;
            }
            if id == crate::compact_layout::TOGGLE && notify == BN_CLICKED {
                e.preview_visible = !e.preview_visible;
                e.layout_applied = false;
                e.resize_view();
                set(
                    hwnd,
                    id,
                    if e.preview_visible {
                        "Hide preview"
                    } else {
                        "Show preview"
                    },
                );
                return 0;
            }
            if notify != BN_CLICKED {
                return 0;
            }
            // OS shortcuts are immediate actions, separate from draft validation,
            // Save, history and unsaved-state tracking.
            if matches!(id, STARTUP | START_MENU) {
                let was_enabled = if id == STARTUP {
                    super::startup::enabled()
                } else {
                    super::startup::start_menu_enabled()
                };
                let result = if id == STARTUP {
                    super::startup::set(!was_enabled)
                } else {
                    super::startup::set_start_menu(!was_enabled)
                };
                let enabled = if id == STARTUP {
                    super::startup::enabled()
                } else {
                    super::startup::start_menu_enabled()
                };
                set(hwnd, id, integration_label(id, enabled));
                e.status = match result {
                    Err(message) => message,
                    Ok(()) => format!(
                        "{}. Applied immediately; no Save needed.",
                        match (id == STARTUP, enabled) {
                            (true, true) => "Windows startup enabled",
                            (true, false) => "Windows startup disabled",
                            (false, true) => "Added to Start menu",
                            (false, false) => "Removed from Start menu",
                        }
                    ),
                };
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            e.checkpoint_settings();
            let result = match id {
                ICON_BROWSE => (|| {
                    let software = send(hwnd, ICON_MODE, CB_GETCURSEL, 0, 0) == 3;
                    let pick = if software { Pick::Software } else { Pick::Icon };
                    if let Some(path) = pickers::choose(hwnd, &e.root, pick)? {
                        e.icon_draft = if software {
                            ItemIcon::Software {
                                path: editing::portable_path(&e.root, &path),
                            }
                        } else {
                            ItemIcon::Custom {
                                path: crate::icon_assets::import(&e.root, &path)?,
                            }
                        };
                        send(
                            hwnd,
                            ICON_MODE,
                            CB_SETCURSEL,
                            if software { 3 } else { 2 },
                            0,
                        );
                        e.enable_fields();
                        e.item_dirty = true;
                        e.status = format!(
                            "Icon source: {}. Apply item, then Save.",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        );
                    }
                    Ok(())
                })(),
                BROWSE_TARGET | BROWSE_RUNNER | BROWSE_CWD => e.browse(id),
                UNDO => e.travel(false),
                REDO => e.travel(true),
                IMPORT => e.transfer(false),
                EXPORT => e.transfer(true),
                APPLY => e.apply(),
                ADD_STAR => e.add(false),
                ADD_FOLDER => e.add(true),
                DUPLICATE => e.duplicate(),
                ADD_UNIVERSE => (|| {
                    e.apply()?;
                    let mut c = e.c.clone();
                    let id = c
                        .max_node_id()
                        .checked_add(1)
                        .ok_or("Item ID space exhausted")?;
                    c.universes.push(Node {
                        browser: None,
                        note: String::new(),
                        icon: Default::default(),
                        visible_in: vec![],
                        id,
                        name: "New universe".into(),
                        children: Some(vec![]),
                        target: None,
                    });
                    e.commit(c);
                    e.universe = e.c.universes.len() - 1;
                    e.selected = id;
                    e.refresh();
                    e.status =
                        "Universe added. Point a shortcut profile at it so a binding opens it."
                            .into();
                    Ok(())
                })(),
                DELETE => (|| {
                    e.apply()?;
                    if e.selected == e.c.universes[e.universe].id {
                        if e.c.universes.len() == 1 {
                            return Err("The last universe cannot be deleted".into());
                        }
                        let doomed = e.c.universes[e.universe].id;
                        if let Some(p) =
                            e.c.profiles
                                .iter()
                                .find(|p| e.c.universe_of(p).is_some_and(|u| u.id == doomed))
                        {
                            return Err(format!(
                                "{} opens this universe; point it elsewhere first",
                                p.name
                            ));
                        }
                        let mut c = e.c.clone();
                        c.universes.remove(e.universe);
                        e.commit(c);
                        e.universe = 0;
                        e.selected = e.c.universes[0].id;
                        e.refresh();
                        e.status = "Universe deleted from draft. Undo restores it.".into();
                        return Ok(());
                    }
                    editing::check_delete(&e.c, e.selected)?;
                    let mut c = e.c.clone();
                    remove(&mut c.universes[e.universe], e.selected);
                    e.commit(c);
                    e.selected = e.c.universes[e.universe].id;
                    e.refresh();
                    e.status =
                        "Deleted from draft. Undo restores the item and its children.".into();
                    Ok(())
                })(),
                UP => e.reorder(false),
                DOWN => e.reorder(true),
                SAVE | 1 => e.save(),
                BROWSER => {
                    open_aux(e, AuxKind::Browser(e.selected));
                    Ok(())
                }
                RENAME => {
                    e.scroll_to(e.scroll_x, 0);
                    SetFocus(dlg(hwnd, NAME));
                    send(hwnd, NAME, EM_SETSEL, 0, -1);
                    Ok(())
                }
                RELOAD => (|| {
                    if (e.dirty || e.item_dirty || e.settings_invalid)
                        && MessageBoxW(
                            hwnd,
                            wide("Discard unsaved edits and reload from disk?").as_ptr(),
                            wide("Reload configuration").as_ptr(),
                            MB_YESNO | MB_ICONQUESTION,
                        ) != IDYES
                    {
                        return Ok(());
                    }
                    let baseline = storage::revision(&e.root)?;
                    let c = storage::read(&e.root)?;
                    if storage::revision(&e.root)? != baseline {
                        return Err("Configuration changed during Reload; please retry".into());
                    }
                    e.disk_revision = Ok(baseline);
                    e.recovery = None;
                    e.icons.invalidate();
                    e.preview_cache = None;
                    if let Some(worker) = &e.advice_worker {
                        worker.invalidate();
                    }
                    e.commit(c.clone());
                    e.saved = c;
                    e.dirty = false;
                    e.refresh();
                    e.settings_to_controls();
                    e.status =
                        "Reloaded portable configuration. Undo restores the previous draft.".into();
                    Ok(())
                })(),
                RECOVER_DRAFT => {
                    open_aux(e, AuxKind::Recovery);
                    Ok(())
                }
                id if matches!(
                    id as u32,
                    super::editor_tools::THEME
                        | super::editor_tools::MOVE
                        | super::editor_tools::PATHS
                ) =>
                {
                    e.open_tool(id as u32)
                }
                PROFILES => {
                    profiles(e);
                    Ok(())
                }
                ADVANCED => {
                    if let Err(message) = e.apply().and_then(|_| e.controls_to_settings()) {
                        e.status = message;
                        InvalidateRect(hwnd, null(), 1);
                        return 0;
                    }
                    e.checkpoint_settings();
                    advanced(e);
                    Ok(())
                }
                _ => Ok(()),
            };
            if let Err(s) = result {
                let _ = std::fs::write(e.root.join("data/editor-error.txt"), &s);
                e.status = s.clone();
                set(hwnd, ADVICE, &s);
            } else {
                e.advice();
            }
            InvalidateRect(hwnd, null(), 1);
            0
        }
        WM_SIZE => {
            trace(match w as u32 {
                SIZE_MAXIMIZED => "resize.maximized",
                SIZE_MINIMIZED => "resize.minimized",
                _ => "resize.restored",
            });
            if w != SIZE_MINIMIZED as usize {
                e.resize_view();
            }
            0
        }
        WM_PAINT => {
            e.update_title();
            trace("paint.begin");
            let measured = std::time::Instant::now();
            let mut sectors = 0;
            render::buffered(hwnd, theme::background(), |dc| {
                theme::logical_dc(dc, e.dpi.0);
                SetViewportOrgEx(
                    dc,
                    -e.dpi.pixels(e.scroll_x),
                    -e.dpi.pixels(e.scroll_y),
                    null_mut(),
                );
                SetViewportOrgEx(dc, -e.dpi.pixels(e.scroll_x), 0, null_mut());
                if e.preview_visible {
                    sectors = e.cached_preview(dc);
                }
                SetViewportOrgEx(dc, 0, 0, null_mut());
                render::fill(
                    dc,
                    &RECT {
                        left: 0,
                        top: (e.form.page_height - 90).max(0),
                        right: e.form.page_width,
                        bottom: e.form.page_height,
                    },
                    theme::background(),
                );
                let mut cr: RECT = zeroed();
                GetClientRect(hwnd, &mut cr);
                render::text(
                    dc,
                    &format!(
                        "{}{}",
                        if e.dirty || e.item_dirty || e.settings_invalid {
                            "Unsaved changes · "
                        } else {
                            "Saved · "
                        },
                        e.status
                    ),
                    15,
                    e.form.page_height - 28,
                    e.dpi.logical(cr.right) - 30,
                    theme::muted(),
                    12,
                );
            });
            trace("paint.end");
            if e.measure_preview && e.preview_data.is_some() {
                e.measure_preview = false;
                let report = serde_json::json!({"sectors":sectors,"paint_ms":measured.elapsed().as_secs_f64()*1000.,
                    "background_prepare_ms":e.preview_data.as_ref().map(|p|p.elapsed_ms),"conditions":"UI paint and background geometry are measured separately; excludes startup, user input and display scanout"});
                let _ = std::fs::write(
                    e.root.join("data/preview-measurement.json"),
                    serde_json::to_vec_pretty(&report).unwrap(),
                );
            }
            0
        }
        WM_CLOSE => {
            if let Some(tool) = e.tools {
                super::focus_window(tool);
                return 0;
            }
            if e.aux.is_some() {
                super::focus_window(e.aux.as_ref().unwrap().hwnd);
                return 0;
            }
            if e.dirty || e.item_dirty || e.settings_invalid {
                open_aux(e, AuxKind::Close);
            } else {
                if !e.has_recovery() {
                    if let Some(writer) = &e.draft_writer {
                        let _ = writer.clear();
                    }
                }
                DestroyWindow(hwnd);
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => {
            // Default non-client handling can run Windows' modal move/size
            // loop. No application-state borrow may span that native loop:
            // its nested events must be able to enter our normal handlers.
            drop(_guard);
            DefWindowProcW(hwnd, msg, w, l)
        }
    }
}
// The owned profile dialog edits a private draft; Done transfers it to the main
// editor and its normal Save persists it. Cancel leaves the main draft untouched.
const P_LIST: i32 = 601;
const P_NAME: i32 = 602;
const P_BINDING: i32 = 603;
const P_MODE: i32 = 604;
const P_HOVER: i32 = 605;
const P_RESTORE: i32 = 606;
const P_ADD: i32 = 607;
const P_REMOVE: i32 = 608;
const P_APPLY: i32 = 609;
const P_DONE: i32 = 610;
const P_RECORD: i32 = 611;
const P_STATUS: i32 = 612;
const P_CANCEL: i32 = 613;
const P_DIRECTIONAL: i32 = 615;
const P_DOT: i32 = 616;
const P_UNIVERSE: i32 = 617;
const P_ENTRY: i32 = 618;
struct Profiles {
    hwnd: HWND,
    draft: Vec<Profile>,
    selected: Option<usize>,
    recording: bool,
    trees: Vec<UniverseInfo>,
}
/// A universe and the constellations a profile may open at, the root first.
struct UniverseInfo {
    id: u64,
    name: String,
    entries: Vec<(u64, String)>,
}
fn collect_entries(n: &Node, depth: usize, out: &mut Vec<(u64, String)>) {
    if n.children.is_none() {
        return;
    }
    out.push((n.id, format!("{}{}", "    ".repeat(depth), n.name)));
    for c in n.children.as_deref().unwrap_or_default() {
        collect_entries(c, depth + 1, out);
    }
}
/// `universe == 0` means the first one, which is how pre-universe profiles read.
fn universe_index(trees: &[UniverseInfo], id: u64) -> usize {
    if id == 0 {
        return 0;
    }
    trees.iter().position(|t| t.id == id).unwrap_or(0)
}
unsafe fn fill_entries(hwnd: HWND, tree: Option<&UniverseInfo>, entry: Option<u64>) {
    unsafe {
        send(hwnd, P_ENTRY, CB_RESETCONTENT, 0, 0);
        let Some(tree) = tree else { return };
        for (i, (_, label)) in tree.entries.iter().enumerate() {
            // Index 0 is the universe root, which is the same as "no entry node".
            let text = if i == 0 {
                format!("{} — whole universe", label.trim())
            } else {
                label.clone()
            };
            send(hwnd, P_ENTRY, CB_ADDSTRING, 0, wide(text).as_ptr() as isize);
        }
        let selected = entry
            .and_then(|id| tree.entries.iter().position(|(e, _)| *e == id))
            .unwrap_or(0);
        send(hwnd, P_ENTRY, CB_SETCURSEL, selected, 0);
    }
}
impl Profiles {
    unsafe fn refresh(&mut self) {
        send(self.hwnd, P_LIST, LB_RESETCONTENT, 0, 0);
        for p in &self.draft {
            send(
                self.hwnd,
                P_LIST,
                LB_ADDSTRING,
                0,
                wide(format!("{} · {}", p.name, p.binding)).as_ptr() as isize,
            );
        }
        if let Some(i) = self.selected {
            send(self.hwnd, P_LIST, LB_SETCURSEL, i, 0);
        }
        self.fields();
    }
    unsafe fn fields(&self) {
        let p = self.selected.and_then(|i| self.draft.get(i));
        for id in [
            P_NAME,
            P_BINDING,
            P_MODE,
            P_HOVER,
            P_RESTORE,
            P_DIRECTIONAL,
            P_DOT,
            P_REMOVE,
            P_APPLY,
            P_RECORD,
            P_UNIVERSE,
            P_ENTRY,
        ] {
            EnableWindow(dlg(self.hwnd, id), p.is_some() as i32);
        }
        set(self.hwnd, P_NAME, p.map_or("", |p| p.name.as_str()));
        set(self.hwnd, P_BINDING, p.map_or("", |p| p.binding.as_str()));
        send(
            self.hwnd,
            P_MODE,
            CB_SETCURSEL,
            usize::from(p.is_some_and(|p| p.mode == Mode::Hold)),
            0,
        );
        send(
            self.hwnd,
            P_HOVER,
            BM_SETCHECK,
            usize::from(p.is_some_and(|p| p.hover)),
            0,
        );
        send(
            self.hwnd,
            P_RESTORE,
            CB_SETCURSEL,
            p.map_or(0, |p| match p.cursor_end {
                CursorEnd::Restore => 0,
                CursorEnd::Center => 1,
                CursorEnd::Leave => 2,
            }),
            0,
        );
        for (id, on) in [
            (P_DIRECTIONAL, p.is_some_and(|p| p.directional)),
            (P_DOT, p.is_some_and(|p| p.dot_cursor)),
        ] {
            send(self.hwnd, id, BM_SETCHECK, usize::from(on), 0);
        }
        let choice = p.map(|p| (p.universe, p.root_id));
        send(self.hwnd, P_UNIVERSE, CB_RESETCONTENT, 0, 0);
        for u in &self.trees {
            send(
                self.hwnd,
                P_UNIVERSE,
                CB_ADDSTRING,
                0,
                wide(&u.name).as_ptr() as isize,
            );
        }
        let index = universe_index(&self.trees, choice.map_or(0, |(u, _)| u));
        send(self.hwnd, P_UNIVERSE, CB_SETCURSEL, index, 0);
        fill_entries(
            self.hwnd,
            self.trees.get(index),
            choice.and_then(|(_, entry)| entry),
        );
    }
    unsafe fn apply(&mut self) -> Result<(), String> {
        let Some(i) = self.selected else {
            return Ok(());
        };
        let name = get(self.hwnd, P_NAME).trim().to_string();
        if name.is_empty() {
            return Err("Give this shortcut profile a name.".into());
        }
        let binding = get(self.hwnd, P_BINDING).trim().to_string();
        let parsed = Binding::parse(&binding)?;
        if self
            .draft
            .iter()
            .enumerate()
            .any(|(j, p)| j != i && Binding::parse(&p.binding).ok().as_ref() == Some(&parsed))
        {
            return Err("That shortcut is already used by another profile.".into());
        }
        let chosen = send(self.hwnd, P_UNIVERSE, CB_GETCURSEL, 0, 0).max(0) as usize;
        let tree = self.trees.get(chosen);
        let entry = send(self.hwnd, P_ENTRY, CB_GETCURSEL, 0, 0).max(0) as usize;
        self.draft[i] = Profile {
            id: self.draft[i].id,
            // Keep the first universe as 0: that is the default, and writing its
            // real id would add a key to profiles that never carried one.
            universe: if chosen == 0 {
                0
            } else {
                tree.map_or(0, |t| t.id)
            },
            // Index 0 of the entry list is the universe root, which is no entry.
            root_id: tree
                .filter(|_| entry > 0)
                .and_then(|t| t.entries.get(entry))
                .map(|(id, _)| *id),
            name,
            binding,
            mode: if send(self.hwnd, P_MODE, CB_GETCURSEL, 0, 0) == 1 {
                Mode::Hold
            } else {
                Mode::Click
            },
            hover: send(self.hwnd, P_HOVER, BM_GETCHECK, 0, 0) == 1,
            cursor_end: match send(self.hwnd, P_RESTORE, CB_GETCURSEL, 0, 0) {
                1 => CursorEnd::Center,
                2 => CursorEnd::Leave,
                _ => CursorEnd::Restore,
            },
            directional: send(self.hwnd, P_DIRECTIONAL, BM_GETCHECK, 0, 0) == 1,
            dot_cursor: send(self.hwnd, P_DOT, BM_GETCHECK, 0, 0) == 1,
        };
        self.refresh();
        Ok(())
    }
    unsafe fn stop_recording(&mut self) {
        self.recording = false;
        super::recorder::stop();
        ReleaseCapture();
        set(self.hwnd, P_RECORD, "Record shortcut");
    }
    unsafe fn capture(&mut self, trigger: &str, modifiers: bool) {
        let mut binding = String::new();
        if modifiers {
            for (vk, label) in [(0x11, "Ctrl+"), (0x10, "Shift+"), (0x12, "Alt+")] {
                if GetKeyState(vk) < 0 {
                    binding.push_str(label);
                }
            }
        }
        binding.push_str(trigger);
        match Binding::parse(&binding) {
            Ok(_) => {
                set(self.hwnd, P_BINDING, &binding);
                self.stop_recording();
                set(
                    self.hwnd,
                    P_STATUS,
                    "Recorded. Apply profile or Done to keep it.",
                );
            }
            Err(e) => set(self.hwnd, P_STATUS, &e),
        }
    }
}
unsafe fn profiles(e: &mut Editor) {
    profiles_inner(e, true);
}
unsafe fn profiles_inner(e: &mut Editor, validate: bool) {
    if let Some(p) = &e.profiles {
        super::focus_window(p.hwnd);
        return;
    }
    // Preserve any current item and spacing edits before opening the modal draft.
    if validate {
        if let Err(err) = e.apply().and_then(|_| e.controls_to_settings()) {
            error(&err);
            return;
        }
    }
    let class = wide("Constellation.Profiles");
    let module = GetModuleHandleW(null());
    let wc = WNDCLASSW {
        hIcon: super::icons::large(),
        lpfnWndProc: Some(profiles_proc),
        hInstance: module,
        lpszClassName: class.as_ptr(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: null_mut(),
        ..zeroed()
    };
    RegisterClassW(&wc);
    // Reserve the intended client area, including the current DPI's title bar
    // and frame rather than clipping the bottom row of controls.
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
    let mut bounds = RECT {
        left: 0,
        top: 0,
        right: Dpi(GetDpiForWindow(e.hwnd)).pixels(810),
        bottom: Dpi(GetDpiForWindow(e.hwnd)).pixels(705),
    };
    AdjustWindowRectExForDpi(&mut bounds, style, 0, 0, GetDpiForWindow(e.hwnd));
    let hwnd = CreateWindowExW(
        WS_EX_COMPOSITED,
        class.as_ptr(),
        wide("Constellation — Shortcut profiles").as_ptr(),
        style,
        100,
        100,
        bounds.right - bounds.left,
        bounds.bottom - bounds.top,
        e.hwnd,
        null_mut(),
        module,
        null(),
    );
    super::icons::set_small(hwnd);
    if hwnd.is_null() {
        error("Cannot create shortcut profile window");
        return;
    }
    theme::window(hwnd);
    let trees =
        e.c.universes
            .iter()
            .map(|u| {
                let mut entries = vec![];
                collect_entries(u, 0, &mut entries);
                UniverseInfo {
                    id: u.id,
                    name: u.name.clone(),
                    entries,
                }
            })
            .collect();
    e.profiles = Some(Profiles {
        hwnd,
        trees,
        draft: e.c.profiles.clone(),
        selected: if e.c.profiles.is_empty() {
            None
        } else {
            Some(0)
        },
        recording: false,
    });
    label(hwnd, "SHORTCUT PROFILES", 20, 18, 260);
    control(
        hwnd,
        "LISTBOX",
        "",
        P_LIST,
        20,
        45,
        260,
        330,
        WS_BORDER | WS_VSCROLL | WS_TABSTOP | LBS_NOTIFY as u32,
    );
    control(hwnd, "BUTTON", "Add", P_ADD, 20, 390, 120, 30, WS_TABSTOP);
    control(
        hwnd, "BUTTON", "Remove", P_REMOVE, 150, 390, 130, 30, WS_TABSTOP,
    );
    for (id, title, y) in [(P_NAME, "Name", 25), (P_BINDING, "Shortcut", 90)] {
        label(hwnd, title, 310, y, 450);
        control(
            hwnd,
            "EDIT",
            "",
            id,
            310,
            y + 23,
            460,
            28,
            WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        );
    }
    control(
        hwnd,
        "BUTTON",
        "Record shortcut",
        P_RECORD,
        310,
        150,
        180,
        30,
        WS_TABSTOP,
    );
    label(
        hwnd,
        "Or type Ctrl+Q, F8, Mouse3, Mouse4, Mouse5…",
        310,
        185,
        470,
    );
    label(hwnd, "Activation", 310, 222, 130);
    control(
        hwnd,
        "COMBOBOX",
        "",
        P_MODE,
        445,
        216,
        325,
        180,
        WS_TABSTOP | CBS_DROPDOWNLIST as u32,
    );
    for name in ["Click — press to open", "Hold — release to select"] {
        send(hwnd, P_MODE, CB_ADDSTRING, 0, wide(name).as_ptr() as isize);
    }
    control(
        hwnd,
        "BUTTON",
        "Open Constellations on hover",
        P_HOVER,
        310,
        265,
        460,
        28,
        WS_TABSTOP | BS_AUTOCHECKBOX as u32,
    );
    label(hwnd, "Cursor on close", 310, 308, 140);
    label(
        hwnd,
        "Opening always centers the menu and cursor.",
        310,
        345,
        470,
    );
    control(
        hwnd,
        "COMBOBOX",
        "",
        P_RESTORE,
        455,
        302,
        315,
        180,
        WS_TABSTOP | CBS_DROPDOWNLIST as u32,
    );
    for name in [
        "Restore original position",
        "Return to menu center",
        "Leave at selection",
    ] {
        send(
            hwnd,
            P_RESTORE,
            CB_ADDSTRING,
            0,
            wide(name).as_ptr() as isize,
        );
    }
    for (id, title, y) in [(
        P_DIRECTIONAL,
        "Select by direction beyond the outer edge",
        382,
    )] {
        control(
            hwnd,
            "BUTTON",
            title,
            id,
            310,
            y,
            460,
            28,
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        );
    }
    control(
        hwnd,
        "BUTTON",
        "Use white-dot cursor while radial is open",
        P_DOT,
        310,
        416,
        470,
        28,
        WS_TABSTOP | BS_AUTOCHECKBOX as u32,
    );
    label(hwnd, "Opens universe", 310, 452, 150);
    control(
        hwnd,
        "COMBOBOX",
        "",
        P_UNIVERSE,
        465,
        446,
        305,
        180,
        WS_TABSTOP | CBS_DROPDOWNLIST as u32,
    );
    label(hwnd, "Opens at", 310, 492, 150);
    control(
        hwnd,
        "COMBOBOX",
        "",
        P_ENTRY,
        465,
        486,
        305,
        180,
        WS_TABSTOP | CBS_DROPDOWNLIST as u32,
    );
    control(
        hwnd,
        "BUTTON",
        "Apply profile",
        P_APPLY,
        310,
        540,
        170,
        32,
        WS_TABSTOP,
    );
    control(
        hwnd,
        "STATIC",
        "Edits follow you when switching profiles. Done keeps changes in the main editor; Save writes them to disk.",
        P_STATUS,
        310,
        580,
        470,
        48,
        0,
    );
    control(
        hwnd,
        "BUTTON",
        "Done",
        P_DONE,
        570,
        650,
        95,
        32,
        WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
    );
    control(
        hwnd, "BUTTON", "Cancel", P_CANCEL, 675, 650, 95, 32, WS_TABSTOP,
    );
    e.profiles.as_mut().unwrap().refresh();
    EnableWindow(e.hwnd, 0);
    if !e.json.is_null() {
        EnableWindow(e.json, 0);
    }
    ShowWindow(hwnd, SW_SHOW);
    super::focus_window(hwnd);
}
unsafe extern "system" fn profiles_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(result) = theme::display_message(hwnd, msg, w, l) {
        return result;
    }
    if msg == WM_COMMAND && !EDITOR.is_null() {
        SetTimer((*EDITOR).hwnd, DRAFT_TIMER, 500, None);
    }
    if let Some(result) = theme::message(msg, w, l) {
        return result;
    }
    if msg == WM_PAINT {
        render::buffered(hwnd, theme::background(), |_| {});
        return 0;
    }
    if EDITOR.is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let Some(_guard) = super::DispatchGuard::enter() else {
        if super::defer_nested_close(hwnd, msg, w, l) {
            return 0;
        }
        if msg == super::recorder::CAPTURE {
            super::defer_message(hwnd, msg, w, l);
            return 0;
        }
        return DefWindowProcW(hwnd, msg, w, l);
    };
    let e = &mut *EDITOR;
    let Some(p) = e.profiles.as_mut() else {
        drop(_guard);
        return DefWindowProcW(hwnd, msg, w, l);
    };
    if msg == super::recorder::CAPTURE {
        if p.recording {
            if GetAsyncKeyState(0x11) < 0
                || GetAsyncKeyState(0x10) < 0
                || GetAsyncKeyState(0x12) < 0
            {
                set(
                    hwnd,
                    P_STATUS,
                    "Mouse shortcuts currently require an unmodified button.",
                );
            } else {
                p.capture(&format!("Mouse{w}"), false);
            }
        }
        return 0;
    }
    if p.recording {
        match msg {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let vk = w as u16;
                if vk == 0x1b {
                    p.stop_recording();
                    set(hwnd, P_STATUS, "Recording cancelled.");
                } else if vk == 0x5b || vk == 0x5c || GetKeyState(0x5b) < 0 || GetKeyState(0x5c) < 0
                {
                    set(
                        hwnd,
                        P_STATUS,
                        "Windows-key shortcuts are not supported in this preview.",
                    );
                } else if !matches!(vk,0x10..=0x12 | 0xa0..=0xa5) {
                    let trigger = match vk {
                        0x30..=0x39 | 0x41..=0x5a => char::from_u32(vk as u32).unwrap().to_string(),
                        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
                        0x20 => "Space".into(),
                        _ => {
                            set(
                                hwnd,
                                P_STATUS,
                                "Use a letter, number, Space, function key, or middle/side mouse button.",
                            );
                            return 0;
                        }
                    };
                    p.capture(&trigger, true);
                }
                return 0;
            }
            WM_MBUTTONDOWN | WM_XBUTTONDOWN => {
                let button = if msg == WM_MBUTTONDOWN {
                    3
                } else if (w >> 16) & 0xffff == 1 {
                    4
                } else {
                    5
                };
                if GetKeyState(0x11) < 0 || GetKeyState(0x10) < 0 || GetKeyState(0x12) < 0 {
                    set(
                        hwnd,
                        P_STATUS,
                        "Mouse shortcuts currently require an unmodified button.",
                    );
                } else {
                    p.capture(&format!("Mouse{button}"), false);
                }
                return 1;
            }
            WM_KILLFOCUS | WM_CANCELMODE => {
                p.stop_recording();
                set(
                    hwnd,
                    P_STATUS,
                    "Recording cancelled. Focus left the recorder.",
                );
            }
            _ => {}
        }
    }
    match msg {
        WM_COMMAND => {
            let id = (w & 0xffff) as i32;
            // The entry list belongs to the chosen universe, so switching
            // universe rebuilds it and falls back to that universe's root.
            if id == P_UNIVERSE && w >> 16 == CBN_SELCHANGE as usize {
                let chosen = send(hwnd, P_UNIVERSE, CB_GETCURSEL, 0, 0).max(0) as usize;
                fill_entries(hwnd, p.trees.get(chosen), None);
                return 0;
            }
            if id == P_LIST && w >> 16 == LBN_SELCHANGE as usize {
                let next = usize::try_from(send(hwnd, P_LIST, LB_GETCURSEL, 0, 0)).ok();
                match p.apply() {
                    Ok(()) => {
                        p.selected = next;
                        if let Some(i) = next {
                            send(hwnd, P_LIST, LB_SETCURSEL, i, 0);
                        }
                        p.fields();
                    }
                    Err(message) => {
                        if let Some(i) = p.selected {
                            send(hwnd, P_LIST, LB_SETCURSEL, i, 0);
                        }
                        set(hwnd, P_STATUS, &message);
                    }
                }
                return 0;
            }
            if w >> 16 != BN_CLICKED as usize {
                return 0;
            }
            let result = match id {
                P_RECORD => (|| {
                    super::recorder::start(hwnd)?;
                    p.recording = true;
                    set(hwnd, P_RECORD, "Listening… Esc cancels");
                    set(
                        hwnd,
                        P_STATUS,
                        "Press your shortcut. For mouse buttons, click inside this window.",
                    );
                    SetFocus(hwnd);
                    SetCapture(hwnd);
                    Ok(())
                })(),
                P_APPLY => p.apply(),
                P_ADD => (|| {
                    p.apply()?;
                    let binding = (1..=24)
                        .filter(|n| *n != 12)
                        .map(|n| format!("F{n}"))
                        .find(|b| {
                            p.draft
                                .iter()
                                .all(|p| Binding::parse(&p.binding).ok() != Binding::parse(b).ok())
                        })
                        .ok_or(
                            "No unused function key available. Edit an existing profile first.",
                        )?;
                    let id = p
                        .draft
                        .iter()
                        .map(|p| p.id)
                        .max()
                        .unwrap_or(0)
                        .checked_add(1)
                        .ok_or("Profile ID space exhausted")?;
                    p.draft.push(Profile {
                        id,
                        name: "New shortcut".into(),
                        binding,
                        mode: Mode::Click,
                        hover: true,
                        cursor_end: CursorEnd::Restore,
                        directional: true,
                        dot_cursor: true,
                        universe: 0,
                        root_id: None,
                    });
                    p.selected = Some(p.draft.len() - 1);
                    p.refresh();
                    Ok(())
                })(),
                P_REMOVE => {
                    if let Some(i) = p.selected {
                        p.draft.remove(i);
                    }
                    p.selected = if p.draft.is_empty() { None } else { Some(0) };
                    p.refresh();
                    Ok(())
                }
                P_DONE | 1 => {
                    (|| {
                        p.apply()?;
                        let mut candidate = e.c.clone();
                        candidate.profiles = p.draft.clone();
                        candidate.validate()?;
                        e.history.record(&e.c, &candidate);
                        e.c = candidate;
                        e.dirty = e.c != e.saved;
                        e.status = "Shortcut profiles updated. Save to apply them to the resident launcher.".into();
                        PostMessageW(hwnd, WM_CLOSE, 0, 0);
                        Ok(())
                    })()
                }
                P_CANCEL | 2 => {
                    PostMessageW(hwnd, WM_CLOSE, 0, 0);
                    Ok(())
                }
                _ => Ok(()),
            };
            if let Err(err) = result {
                set(hwnd, P_STATUS, &err);
            } else if id == P_APPLY {
                set(
                    hwnd,
                    P_STATUS,
                    "Profile applied to this draft. Done keeps it; Save persists it.",
                );
            }
            0
        }
        WM_CLOSE => {
            p.stop_recording();
            EnableWindow(e.hwnd, 1);
            if !e.json.is_null() {
                EnableWindow(e.json, 1);
            }
            DestroyWindow(hwnd);
            e.profiles = None;
            if IsWindowVisible(e.hwnd) != 0 {
                SetForegroundWindow(e.hwnd);
            }
            InvalidateRect(e.hwnd, null(), 1);
            0
        }
        _ => {
            // Default non-client handling can run Windows' modal move/size
            // loop. No application-state borrow may span that native loop:
            // its nested events must be able to enter our normal handlers.
            drop(_guard);
            DefWindowProcW(hwnd, msg, w, l)
        }
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    static NATIVE_EDITOR: std::sync::Mutex<()> = std::sync::Mutex::new(());
    #[test]
    #[ignore = "hidden compact editor pane regression; Windows CI only"]
    fn compact_panes_keep_scroll_context_and_unapplied_text() {
        let _serial = NATIVE_EDITOR.lock().unwrap();
        std::thread::spawn(|| unsafe {
            theme::install();
            let module = GetModuleHandleW(null());
            let class = wide("Constellation.Test.CompactPanes");
            assert_ne!(
                RegisterClassW(&WNDCLASSW {
                    lpfnWndProc: Some(proc),
                    hInstance: module,
                    lpszClassName: class.as_ptr(),
                    ..zeroed()
                }),
                0
            );
            let owner = CreateWindowExW(
                0,
                class.as_ptr(),
                wide("hidden panes").as_ptr(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                900,
                650,
                null_mut(),
                null_mut(),
                module,
                null(),
            );
            assert!(!owner.is_null());
            ROOT_WINDOW.with(|root| root.set(owner));
            let item = super::super::editor_pane::create(owner, crate::compact_layout::ITEM);
            let side = super::super::editor_pane::create(owner, crate::compact_layout::SIDE);
            assert!(!item.is_null() && !side.is_null());
            control(
                owner,
                "EDIT",
                "unfinished name",
                NAME,
                305,
                83,
                360,
                28,
                WS_TABSTOP,
            );
            control(
                owner,
                "EDIT",
                "invalid radius",
                RADIUS,
                305,
                735,
                80,
                28,
                WS_TABSTOP,
            );
            control(owner, "BUTTON", "Save", SAVE, 20, 906, 100, 35, WS_TABSTOP);
            let controls = NEW_CONTROLS.with(|items| std::mem::take(&mut *items.borrow_mut()));
            for c in &controls {
                if crate::compact_layout::item(c.base) {
                    SetParent(c.hwnd, item);
                }
            }
            let config = Config::default();
            let root = std::env::current_dir()
                .unwrap()
                .join("target/compact-fixtures")
                .join(format!(
                    "{}-{}",
                    std::process::id(),
                    windows_sys::Win32::System::Threading::GetCurrentThreadId()
                ));
            let mut editor = Editor::new(owner, root, config.clone(), vec![]);
            editor.controls = controls;
            editor.item_dirty = true;
            editor.settings_invalid = true;
            EDITOR = Box::into_raw(Box::new(editor));
            (*EDITOR).resize_view();
            let name = dlg(owner, NAME);
            assert_eq!(GetParent(name), item);
            (*EDITOR).scroll_pane(false, SB_BOTTOM, None);
            let sidebar_position = (*EDITOR).scroll_y;
            (*EDITOR).scroll_pane(true, SB_BOTTOM, None);
            assert!((*EDITOR).item_scroll > 0);
            let item_position = (*EDITOR).item_scroll;
            assert_eq!((*EDITOR).scroll_y, sidebar_position);
            SendMessageW(owner, super::super::editor_pane::REVEAL, 0, name as isize);
            assert!((*EDITOR).item_scroll < item_position);
            let (mut field_rect, mut pane_rect): (RECT, RECT) = (zeroed(), zeroed());
            GetWindowRect(name, &mut field_rect);
            GetWindowRect(item, &mut pane_rect);
            assert!(field_rect.top >= pane_rect.top && field_rect.bottom <= pane_rect.bottom);
            assert_eq!((*EDITOR).scroll_y, sidebar_position);
            SendMessageW(owner, WM_COMMAND, crate::compact_layout::TOGGLE as usize, 0);
            assert!(!(*EDITOR).preview_visible);
            assert!(!(*EDITOR).form.horizontal);
            assert_eq!(get(owner, NAME), "unfinished name");
            assert_eq!(get(owner, RADIUS), "invalid radius");
            assert!((*EDITOR).item_dirty && (*EDITOR).settings_invalid);
            assert_eq!(
                (*EDITOR).c,
                config,
                "Preview toggle must not apply or discard fields"
            );
            let mut bounds: RECT = zeroed();
            GetWindowRect(dlg(owner, SAVE), &mut bounds);
            let mut client: RECT = zeroed();
            GetClientRect(owner, &mut client);
            let mut bottom = POINT {
                x: bounds.right,
                y: bounds.bottom,
            };
            ScreenToClient(owner, &mut bottom);
            assert!(bottom.y <= client.bottom && bottom.x <= client.right);
            DestroyWindow(owner);
            drop(Box::from_raw(EDITOR));
            EDITOR = null_mut();
            ROOT_WINDOW.with(|root| root.set(null_mut()));
            theme::shutdown();
            UnregisterClassW(class.as_ptr(), module);
        })
        .join()
        .unwrap();
    }
    #[test]
    #[ignore = "hidden browser and close-dialog interactions; Windows CI only"]
    fn browser_and_close_dialogs_preserve_drafts_and_guard_saves() {
        let _serial = NATIVE_EDITOR.lock().unwrap();
        std::thread::spawn(|| unsafe {
            theme::install();
            let module = GetModuleHandleW(null());
            let class = wide("Constellation.Test.ItemDialogs");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: module,
                lpszClassName: class.as_ptr(),
                ..zeroed()
            };
            assert_ne!(RegisterClassW(&wc), 0);
            for action in [AUX_APPLY, AUX_DISCARD] {
                let root = std::env::current_dir()
                    .unwrap()
                    .join("target/editor-dialog-tests")
                    .join(format!(
                        "{}-{action}-{}",
                        std::process::id(),
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_nanos()
                    ));
                let original = Config::default();
                storage::save(&root, &original).unwrap();
                let owner = CreateWindowExW(
                    0,
                    class.as_ptr(),
                    wide("hidden item dialogs").as_ptr(),
                    WS_OVERLAPPEDWINDOW,
                    0,
                    0,
                    800,
                    600,
                    null_mut(),
                    null_mut(),
                    module,
                    null(),
                );
                assert!(!owner.is_null());
                for id in [
                    NAME, NOTE, TARGET, ARGS, RUNNER, CWD, RADIUS, DISTANCE, SPREAD, GAP, VOLUME,
                ] {
                    control(owner, "EDIT", "", id, 0, 0, 200, 25, 0);
                }
                control(
                    owner,
                    "COMBOBOX",
                    "",
                    INSTRUMENT,
                    0,
                    0,
                    200,
                    100,
                    CBS_DROPDOWNLIST as u32,
                );
                for choice in ["None", "Mallet", "Bell", "Plucked", "Chimes"] {
                    send(
                        owner,
                        INSTRUMENT,
                        CB_ADDSTRING,
                        0,
                        wide(choice).as_ptr() as isize,
                    );
                }
                browser_header(owner);
                let browser_button = dlg(owner, BROWSER);
                let caption = FindWindowExW(
                    owner,
                    null_mut(),
                    wide("STATIC").as_ptr(),
                    wide("Target path / URL").as_ptr(),
                );
                assert!(!caption.is_null());
                let (mut caption_rect, mut button_rect): (RECT, RECT) = (zeroed(), zeroed());
                GetWindowRect(caption, &mut caption_rect);
                GetWindowRect(browser_button, &mut button_rect);
                assert!(
                    caption_rect.right < button_rect.left,
                    "Caption overlaps the browser button"
                );
                assert_eq!(
                    ChildWindowFromPoint(owner, POINT { x: 550, y: 180 }),
                    browser_button
                );
                unsafe fn assert_centered(owner: HWND, dialog: HWND) {
                    let (mut a, mut b): (RECT, RECT) = (zeroed(), zeroed());
                    GetWindowRect(owner, &mut a);
                    GetWindowRect(dialog, &mut b);
                    let mut info: MONITORINFO = zeroed();
                    info.cbSize = size_of::<MONITORINFO>() as u32;
                    assert_ne!(
                        GetMonitorInfoW(
                            MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST),
                            &mut info
                        ),
                        0
                    );
                    let rect = |r: RECT| FormRect {
                        x: r.left,
                        y: r.top,
                        w: r.right - r.left,
                        h: r.bottom - r.top,
                    };
                    let (x, y) = crate::editor_layout::dialog_position(
                        rect(a),
                        rect(info.rcWork),
                        b.right - b.left,
                        b.bottom - b.top,
                    );
                    assert_eq!((b.left, b.top), (x, y));
                }
                EDITOR = Box::into_raw(Box::new(Editor::new(
                    owner,
                    root.clone(),
                    original.clone(),
                    vec![],
                )));
                (*EDITOR).disk_revision = Ok(storage::revision(&root).unwrap());
                {
                    let _g = super::super::DispatchGuard::enter().unwrap();
                    (*EDITOR).settings_to_controls();
                    open_aux(&mut *EDITOR, AuxKind::Browser(original.universes[0].id));
                }
                let dialog = (*EDITOR).aux.as_ref().unwrap().hwnd;
                assert_centered(owner, dialog);
                assert_eq!(IsWindowVisible(dialog), 0);
                assert_eq!(IsWindowEnabled(owner), 0);
                send(dialog, AUX_MODE, CB_SETCURSEL, 1, 0);
                set(dialog, AUX_PATH, "test-browser.exe");
                set(dialog, AUX_ARGS, "--work");
                SendMessageW(dialog, WM_COMMAND, AUX_APPLY as usize, 0);
                assert!((*EDITOR).aux.is_none());
                assert_ne!(IsWindowEnabled(owner), 0);
                assert!(matches!(
                    (&(*EDITOR).c.universes)[0].browser,
                    Some(Browser::Program { .. })
                ));
                let draft = (*EDITOR).c.clone();
                {
                    let _g = super::super::DispatchGuard::enter().unwrap();
                    open_aux(&mut *EDITOR, AuxKind::Browser(original.universes[0].id));
                }
                let dialog = (*EDITOR).aux.as_ref().unwrap().hwnd;
                assert_centered(owner, dialog);
                set(dialog, AUX_PATH, "discarded-browser.exe");
                SendMessageW(dialog, WM_CLOSE, 0, 0);
                assert_eq!((*EDITOR).c, draft);
                SendMessageW(owner, WM_CLOSE, 0, 0);
                let dialog = (*EDITOR).aux.as_ref().unwrap().hwnd;
                assert_centered(owner, dialog);
                SendMessageW(dialog, WM_COMMAND, AUX_CANCEL as usize, 0);
                assert_ne!(IsWindow(owner), 0);
                assert_eq!((*EDITOR).c, draft);
                assert_eq!(storage::read(&root).unwrap(), original);
                let mut external = original.clone();
                external.universes[0].name = "External".into();
                storage::save(&root, &external).unwrap();
                SendMessageW(owner, WM_CLOSE, 0, 0);
                let dialog = (*EDITOR).aux.as_ref().unwrap().hwnd;
                assert_centered(owner, dialog);
                SendMessageW(dialog, WM_COMMAND, AUX_APPLY as usize, 0);
                assert_ne!(IsWindow(owner), 0);
                assert!((*EDITOR).aux.is_some());
                assert!(get(dialog, AUX_STATUS).contains("changed outside"));
                assert_eq!(storage::read(&root).unwrap(), external);
                SendMessageW(dialog, WM_COMMAND, AUX_CANCEL as usize, 0);
                (*EDITOR).disk_revision = Ok(storage::revision(&root).unwrap());
                SendMessageW(owner, WM_CLOSE, 0, 0);
                let dialog = (*EDITOR).aux.as_ref().unwrap().hwnd;
                assert_centered(owner, dialog);
                SendMessageW(dialog, WM_COMMAND, action as usize, 0);
                assert_eq!(IsWindow(owner), 0);
                assert_eq!(
                    storage::read(&root).unwrap(),
                    if action == AUX_APPLY { draft } else { external }
                );
                drop(Box::from_raw(EDITOR));
                EDITOR = null_mut();
            }
            theme::shutdown();
        })
        .join()
        .unwrap();
    }

    #[test]
    #[ignore = "hidden large native tree measurement; run on Windows CI"]
    fn large_tree_rename_updates_text_without_recreating_items() {
        let _serial = NATIVE_EDITOR.lock().unwrap();
        std::thread::spawn(|| unsafe {
            let common = INITCOMMONCONTROLSEX {
                dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_TREEVIEW_CLASSES,
            };
            assert_ne!(InitCommonControlsEx(&common), 0);
            let owner = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("tree measurement").as_ptr(),
                WS_POPUP,
                0,
                0,
                800,
                500,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            );
            assert!(!owner.is_null());
            let tree = control(
                owner,
                "SysTreeView32",
                "",
                TREE,
                0,
                0,
                260,
                300,
                TVS_HASBUTTONS | TVS_HASLINES | TVS_LINESATROOT,
            );
            let mut config = Config::default();
            let sample = config.universes[0]
                .children
                .as_ref()
                .unwrap()
                .iter()
                .find(|n| n.children.is_none())
                .unwrap()
                .clone();
            config.universes[0].children = Some(
                (0..10000)
                    .map(|i| {
                        let mut n = sample.clone();
                        n.id = 100 + i;
                        n.name = format!("Star {i}");
                        n.icon = ItemIcon::Symbol;
                        n
                    })
                    .collect(),
            );
            let mut editor = Editor::new(owner, PathBuf::new(), config, vec![]);
            let start = std::time::Instant::now();
            editor.refresh();
            let build = start.elapsed();
            let handles = editor.tree_items.clone();
            editor.c.universes[0].find_mut(500).unwrap().name = "Renamed".into();
            let start = std::time::Instant::now();
            editor.refresh();
            let rename = start.elapsed();
            assert_eq!(editor.tree_items, handles);
            let mut text = [0u16; 256];
            let mut item = TVITEMW {
                mask: TVIF_TEXT,
                hItem: editor.tree_handle(500).unwrap(),
                pszText: text.as_mut_ptr(),
                cchTextMax: text.len() as i32,
                ..zeroed()
            };
            assert_ne!(
                SendMessageW(tree, TVM_GETITEMW, 0, &mut item as *mut _ as isize),
                0
            );
            assert!(String::from_utf16_lossy(&text).contains("Renamed"));
            let report = serde_json::json!({"items":10001,"build_ms":build.as_secs_f64()*1000.,"rename_ms":rename.as_secs_f64()*1000.,"recreated_items":0,"conditions":"Hidden native Windows tree, debug build; includes view capture and field refresh, excludes preview rasterization and display scanout"});
            std::fs::create_dir_all(".local/render-review").unwrap();
            std::fs::write(".local/render-review/tree-refresh.json", serde_json::to_vec_pretty(&report).unwrap()).unwrap();
            drop(editor);
            DestroyWindow(owner);
        })
        .join()
        .unwrap();
    }

    #[test]
    #[ignore = "offscreen native preview regression; run on Windows CI"]
    fn switching_universes_rebuilds_cached_preview_without_editing_config() {
        std::thread::spawn(|| unsafe {
            let mut config = Config::default();
            let mut second = config.universes[0].clone();
            second.id = 10000;
            second.name = "Empty second universe".into();
            second.children = Some(vec![]);
            config.universes.push(second);
            fn symbols(node: &mut Node) {
                node.icon = ItemIcon::Symbol;
                for child in node.children.iter_mut().flatten() {
                    symbols(child);
                }
            }
            for root in &mut config.universes {
                symbols(root);
            }
            let mut editor = Editor::new(null_mut(), PathBuf::new(), config.clone(), vec![]);
            // Exercise cache invalidation/reuse independently from background worker timing.
            editor.preview_worker = None;
            let dc = CreateCompatibleDC(null_mut());
            assert!(!dc.is_null());
            let bitmap = CreateCompatibleBitmap(dc, 1400, 900);
            assert!(!bitmap.is_null());
            let previous = SelectObject(dc, bitmap);
            let preview_model = editor.preview_model.clone();
            let layout = editor.c.layout;
            let theme = editor.c.theme;
            let prepare = |universe| {
                crate::preview::prepare(&crate::preview::Request {
                    config: preview_model.clone(),
                    universe,
                    layout,
                    theme,
                    width: 1920.,
                    height: 1080.,
                })
            };
            editor.preview_data = Some(prepare(0));
            let first = editor.cached_preview(dc);
            assert!(first > 0);
            let cached = editor.preview_cache.as_ref().unwrap().bitmap;
            assert_eq!(editor.cached_preview(dc), first);
            assert_eq!(editor.preview_cache.as_ref().unwrap().bitmap, cached);
            editor.universe = 1;
            editor.selected = config.universes[1].id;
            editor.preview_data = Some(prepare(1));
            assert_eq!(
                editor.cached_preview(dc),
                0,
                "old universe pixels were reused"
            );
            assert_eq!(editor.preview_cache.as_ref().unwrap().universe, 1);
            assert_eq!(editor.preview_cache.as_ref().unwrap().prepared.universe, 1);
            editor.universe = 0;
            editor.selected = config.universes[0].id;
            editor.preview_data = Some(prepare(0));
            assert_eq!(editor.cached_preview(dc), first);
            assert_eq!(editor.preview_cache.as_ref().unwrap().universe, 0);
            assert_eq!(editor.c, config);
            assert!(!editor.dirty && !editor.item_dirty);
            drop(editor);
            SelectObject(dc, previous);
            DeleteObject(bitmap);
            DeleteDC(dc);
        })
        .join()
        .unwrap();
    }

    #[test]
    #[ignore = "hidden native tree interactions; run on Windows CI"]
    fn tree_preserves_context_and_validates_before_native_selection() {
        let _serial = NATIVE_EDITOR.lock().unwrap();
        std::thread::spawn(|| unsafe {
            let common = INITCOMMONCONTROLSEX {
                dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_TREEVIEW_CLASSES,
            };
            assert_ne!(InitCommonControlsEx(&common), 0);
            let module = GetModuleHandleW(null());
            let class = wide("Constellation.Test.Tree");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: module,
                lpszClassName: class.as_ptr(),
                ..zeroed()
            };
            assert_ne!(RegisterClassW(&wc), 0);
            // The owner remains hidden for the entire test. No input synthesis.
            let owner = CreateWindowExW(
                0,
                class.as_ptr(),
                wide("tree test").as_ptr(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                800,
                500,
                null_mut(),
                null_mut(),
                module,
                null(),
            );
            assert!(!owner.is_null());
            theme::install();
            let tree = control(
                owner,
                "SysTreeView32",
                "",
                TREE,
                0,
                0,
                260,
                90,
                TVS_HASBUTTONS | TVS_HASLINES | TVS_LINESATROOT | TVS_SHOWSELALWAYS,
            );
            for id in [NAME, NOTE, TARGET, ARGS, RUNNER, CWD, ITEM_PATH] {
                control(owner, "EDIT", "", id, 300, 10, 300, 25, 0);
            }
            for id in [KIND, PARENT, ICON_MODE, UNIVERSE] {
                control(
                    owner,
                    "COMBOBOX",
                    "",
                    id,
                    300,
                    40,
                    300,
                    100,
                    CBS_DROPDOWNLIST as u32,
                );
            }
            for label in [
                "Constellation",
                "Program",
                "Shortcut",
                "URL",
                "Folder",
                "Script",
            ] {
                send(owner, KIND, CB_ADDSTRING, 0, wide(label).as_ptr() as isize);
            }
            for label in ["Automatic", "Symbol", "Image", "Software"] {
                send(
                    owner,
                    ICON_MODE,
                    CB_ADDSTRING,
                    0,
                    wide(label).as_ptr() as isize,
                );
            }
            control(
                owner,
                "LISTBOX",
                "",
                VISIBLE,
                300,
                80,
                200,
                90,
                LBS_MULTIPLESEL as u32,
            );
            control(owner, "STATIC", "", ADVICE, 300, 180, 300, 80, 0);
            control(owner, "STATIC", "", NOTE_COUNT, 300, 260, 300, 20, 0);
            for (id, title, rect) in crate::editor_layout::CREATE_CONTROLS {
                let button = control(
                    owner, "BUTTON", title, id, rect.x, rect.y, rect.w, rect.h, 0,
                );
                let dc = GetDC(button);
                let font = SendMessageW(button, WM_GETFONT, 0, 0);
                let old_font = SelectObject(dc, font as _);
                let text = wide(title);
                let mut extent: SIZE = zeroed();
                assert_ne!(
                    GetTextExtentPoint32W(dc, text.as_ptr(), text.len() as i32 - 1, &mut extent),
                    0
                );
                assert!(
                    extent.cx + 6 <= rect.w,
                    "Creation button label is clipped: {title}"
                );
                SelectObject(dc, old_font);
                ReleaseDC(button, dc);
            }
            let mut config = Config::default();
            fn symbols(n: &mut Node) {
                n.icon = ItemIcon::Symbol;
                for child in n.children.iter_mut().flatten() {
                    symbols(child);
                }
            }
            symbols(&mut config.universes[0]);
            let mut second = config.universes[0].clone();
            fn renumber(n: &mut Node) {
                n.id += 1000;
                for child in n.children.iter_mut().flatten() {
                    renumber(child);
                }
            }
            renumber(&mut second);
            config.universes.push(second);
            EDITOR = Box::into_raw(Box::new(Editor::new(
                owner,
                PathBuf::new(),
                config.clone(),
                vec![],
            )));
            {
                let _guard = super::super::DispatchGuard::enter().unwrap();
                (*EDITOR).refresh();
            }
            let state = |id| {
                let mut item = TVITEMW {
                    mask: TVIF_STATE,
                    hItem: (*EDITOR).tree_handle(id).unwrap(),
                    stateMask: TVIS_EXPANDED,
                    ..zeroed()
                };
                SendMessageW(tree, TVM_GETITEMW, 0, &mut item as *mut _ as isize);
                item.state & TVIS_EXPANDED != 0
            };
            assert!(state(1));
            assert!(!state(2));
            SendMessageW(
                tree,
                TVM_EXPAND,
                TVE_EXPAND as usize,
                (*EDITOR).tree_handle(2).unwrap(),
            );
            assert_eq!((*EDITOR).selected, 1, "Expansion button selected the row");
            SendMessageW(
                tree,
                TVM_EXPAND,
                TVE_EXPAND as usize,
                (*EDITOR).tree_handle(5).unwrap(),
            );
            assert!(!(*EDITOR).dirty && !(*EDITOR).item_dirty);
            SendMessageW(
                tree,
                TVM_SELECTITEM,
                TVGN_CARET as usize,
                (*EDITOR).tree_handle(6).unwrap(),
            );
            assert_eq!((*EDITOR).selected, 6);
            let expected = editing::node_path(&config.universes[0], 6)
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>()
                .join(" › ");
            assert_eq!(get(owner, ITEM_PATH), expected);
            // User can keep a different sibling visible while editing this node.
            SendMessageW(
                tree,
                TVM_SELECTITEM,
                TVGN_FIRSTVISIBLE as usize,
                (*EDITOR).tree_handle(5).unwrap(),
            );
            let retained_handles = (*EDITOR).tree_items.clone();
            let top = (*EDITOR).tree_id(SendMessageW(
                tree,
                TVM_GETNEXTITEM,
                TVGN_FIRSTVISIBLE as usize,
                0,
            ));
            {
                let _guard = super::super::DispatchGuard::enter().unwrap();
                (*EDITOR).refresh();
            }
            assert_eq!((*EDITOR).selected, 6);
            assert_eq!(
                (*EDITOR).tree_items,
                retained_handles,
                "Unchanged trees retain native handles"
            );
            assert!(state(2) && state(5));
            assert_eq!(
                (*EDITOR).tree_id(SendMessageW(
                    tree,
                    TVM_GETNEXTITEM,
                    TVGN_FIRSTVISIBLE as usize,
                    0
                )),
                top
            );
            set(owner, NAME, "");
            SendMessageW(
                tree,
                TVM_SELECTITEM,
                TVGN_CARET as usize,
                (*EDITOR).tree_handle(8).unwrap(),
            );
            assert_eq!((*EDITOR).selected, 6, "Invalid edit was lost on selection");
            assert_eq!(get(owner, NAME), "");
            set(owner, NAME, "Renamed child");
            SendMessageW(
                tree,
                TVM_SELECTITEM,
                TVGN_CARET as usize,
                (*EDITOR).tree_handle(8).unwrap(),
            );
            assert_eq!((*EDITOR).selected, 8);
            assert!((*EDITOR).tree_refresh_pending);
            assert_eq!((*EDITOR).c.find(6).unwrap().name, "Renamed child");
            SendMessageW(owner, REFRESH_TREE, 0, 0);
            assert!(!(*EDITOR).tree_refresh_pending);
            assert_eq!(
                (*EDITOR).tree_items,
                retained_handles,
                "Renaming retains native handles"
            );
            assert_eq!((*EDITOR).selected, 8);
            assert!(state(2) && state(5));
            // Universe switching restores that universe's own view and selection.
            send(owner, UNIVERSE, CB_SETCURSEL, 1, 0);
            SendMessageW(
                owner,
                WM_COMMAND,
                UNIVERSE as usize | ((CBN_SELCHANGE as usize) << 16),
                dlg(owner, UNIVERSE) as isize,
            );
            assert_eq!((*EDITOR).selected, 1001);
            assert!(state(1001) && !state(1002));
            send(owner, UNIVERSE, CB_SETCURSEL, 0, 0);
            SendMessageW(
                owner,
                WM_COMMAND,
                UNIVERSE as usize | ((CBN_SELCHANGE as usize) << 16),
                dlg(owner, UNIVERSE) as isize,
            );
            assert_eq!((*EDITOR).selected, 8);
            assert!(state(2) && state(5));
            // Collapsing the selected node's ancestor keeps the editor in sync.
            SendMessageW(
                tree,
                TVM_SELECTITEM,
                TVGN_CARET as usize,
                (*EDITOR).tree_handle(6).unwrap(),
            );
            // Exercise the native expand button, not TVM_EXPAND: after the
            // first expansion, that programmatic message omits notifications.
            MoveWindow(tree, 0, 0, 260, 300, 0);
            let mut button = None;
            for y in 0..300 {
                for x in 0..65 {
                    let mut hit = TVHITTESTINFO {
                        pt: POINT { x, y },
                        ..zeroed()
                    };
                    SendMessageW(tree, TVM_HITTEST, 0, &mut hit as *mut _ as isize);
                    if hit.hItem == (*EDITOR).tree_handle(2).unwrap()
                        && hit.flags & TVHT_ONITEMBUTTON != 0
                    {
                        button = Some((x, y));
                        break;
                    }
                }
                if button.is_some() {
                    break;
                }
            }
            let (x, y) = button.expect("Native collapse button is visible");
            let point = x as isize | ((y as isize) << 16);
            SendMessageW(tree, WM_LBUTTONDOWN, 1, point);
            SendMessageW(tree, WM_LBUTTONUP, 0, point);
            assert_eq!((*EDITOR).selected, 2);
            assert!(!state(2));
            let original = (*EDITOR).c.find(2).unwrap().clone();
            let maximum = (*EDITOR).c.max_node_id();
            SendMessageW(
                owner,
                WM_COMMAND,
                DUPLICATE as usize,
                dlg(owner, DUPLICATE) as isize,
            );
            let copy = (*EDITOR).selected;
            assert!(copy > maximum);
            assert_eq!((*EDITOR).c.find(2).unwrap(), &original);
            assert_eq!(
                (*EDITOR)
                    .c
                    .find(copy)
                    .unwrap()
                    .children
                    .as_ref()
                    .unwrap()
                    .len(),
                original.children.as_ref().unwrap().len()
            );
            assert_eq!(get(owner, NAME), format!("{} (copy)", original.name));
            assert!((*EDITOR).dirty);
            assert_eq!(IsWindowVisible(owner), 0);
            drop(Box::from_raw(EDITOR));
            EDITOR = null_mut();
            DestroyWindow(owner);
            UnregisterClassW(class.as_ptr(), module);
            theme::shutdown();
        })
        .join()
        .unwrap();
    }

    // Run on the remote Windows CI runner only. No visible windows, hooks,
    // mouse/keyboard synthesis or user configuration are involved.
    #[test]
    #[ignore = "hidden native window lifecycle; run on Windows CI"]
    fn default_native_resize_and_close_deliver_layout_and_reenable_owner() {
        let _serial = NATIVE_EDITOR.lock().unwrap();
        std::thread::spawn(|| unsafe {
            let module = GetModuleHandleW(null());
            let owner_class = wide("Constellation.Test.OwnerClose");
            let profile_class = wide("Constellation.Test.ProfileClose");
            for (name, procedure) in [
                (
                    owner_class.as_ptr(),
                    Some(proc as unsafe extern "system" fn(_, _, _, _) -> _),
                ),
                (
                    profile_class.as_ptr(),
                    Some(profiles_proc as unsafe extern "system" fn(_, _, _, _) -> _),
                ),
            ] {
                let class = WNDCLASSW {
                    lpfnWndProc: procedure,
                    hInstance: module,
                    lpszClassName: name,
                    ..zeroed()
                };
                assert_ne!(RegisterClassW(&class), 0);
            }
            let owner = CreateWindowExW(
                0,
                owner_class.as_ptr(),
                wide("owner").as_ptr(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                600,
                400,
                null_mut(),
                null_mut(),
                module,
                null(),
            );
            assert!(!owner.is_null());
            let original = Config::default();
            EDITOR = Box::into_raw(Box::new(Editor::new(
                owner,
                PathBuf::new(),
                original.clone(),
                vec![],
            )));
            // WM_WINDOWPOSCHANGED's default processing synchronously sends
            // WM_SIZE, just as native drag/restore processing does. It must
            // reach the actual layout handler immediately, before pumping any
            // posted queue. Holding DispatchGuard across DefWindowProc leaves
            // layout_applied false and defers the size instead.
            assert!(!(*EDITOR).layout_applied);
            let position = WINDOWPOS {
                hwnd: owner,
                hwndInsertAfter: null_mut(),
                x: 0,
                y: 0,
                cx: 600,
                cy: 400,
                flags: SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            };
            SendMessageW(
                owner,
                WM_WINDOWPOSCHANGED,
                0,
                &position as *const WINDOWPOS as isize,
            );
            assert!(
                (*EDITOR).layout_applied,
                "native nested resize was deferred"
            );
            let mut client: RECT = zeroed();
            GetClientRect(owner, &mut client);
            assert_eq!((*EDITOR).form.page_width, client.right);
            assert_eq!((*EDITOR).form.page_height, client.bottom);
            let child = CreateWindowExW(
                0,
                profile_class.as_ptr(),
                wide("profiles").as_ptr(),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
                0,
                0,
                300,
                200,
                owner,
                null_mut(),
                module,
                null(),
            );
            assert!(!child.is_null());
            (*EDITOR).profiles = Some(Profiles {
                hwnd: child,
                draft: original.profiles.clone(),
                selected: None,
                recording: false,
                trees: vec![],
            });
            EnableWindow(owner, 0);
            // This is the real titlebar X / Alt+F4 path, including Windows'
            // synchronous reentry through DefWindowProc into WM_CLOSE.
            SendMessageW(child, WM_SYSCOMMAND, SC_CLOSE as usize, 0);
            let mut message: MSG = zeroed();
            for _ in 0..100 {
                if PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) == 0 {
                    break;
                }
                if message.message != WM_QUIT {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            assert_eq!(IsWindow(child), 0);
            assert_ne!(IsWindowEnabled(owner), 0);
            assert!((*EDITOR).profiles.is_none());
            assert_eq!((*EDITOR).c, original);
            assert!(!(*EDITOR).dirty);
            DestroyWindow(owner);
            drop(Box::from_raw(EDITOR));
            EDITOR = null_mut();
            UnregisterClassW(profile_class.as_ptr(), module);
            UnregisterClassW(owner_class.as_ptr(), module);
        })
        .join()
        .unwrap();
    }
}

#[derive(Clone, Copy)]
enum AuxKind {
    Browser(u64),
    Close,
    Recovery,
}
struct Aux {
    hwnd: HWND,
    kind: AuxKind,
}
const AUX_MODE: i32 = 801;
const AUX_PATH: i32 = 802;
const AUX_ARGS: i32 = 803;
const AUX_BROWSE: i32 = 804;
const AUX_APPLY: i32 = 805;
const AUX_DISCARD: i32 = 806;
const AUX_CANCEL: i32 = 807;
const AUX_STATUS: i32 = 808;
unsafe fn editor_accelerator(msg: &MSG, main: HWND, e: &Editor) -> bool {
    if msg.message != WM_KEYDOWN {
        return false;
    }
    let down = |key: i32| GetKeyState(key) as u16 & 0x8000 != 0;
    let ctrl = down(VK_CONTROL as i32);
    let focus = GetFocus();
    let mut class = [0u16; 64];
    let count = GetClassNameW(focus, class.as_mut_ptr(), class.len() as i32).max(0) as usize;
    let text = String::from_utf16_lossy(&class[..count]).eq_ignore_ascii_case("edit");
    let focus = if text {
        editing::EditorFocus::Text
    } else if focus == dlg(main, TREE) {
        editing::EditorFocus::Tree
    } else {
        editing::EditorFocus::Other
    };
    let command = editing::editor_shortcut(
        msg.wParam as u32,
        ctrl,
        down(VK_SHIFT as i32),
        down(VK_MENU as i32),
        focus,
    );
    let Some(command) = command else {
        return false;
    };
    let root = GetAncestor(msg.hwnd, GA_ROOT);
    let (window, id) = if root == main
        && IsWindowEnabled(main) != 0
        && e.aux.is_none()
        && e.tools.is_none()
        && e.profiles.is_none()
        && e.json.is_null()
    {
        (
            main,
            match command {
                editing::EditorShortcut::Save => SAVE,
                editing::EditorShortcut::Duplicate => DUPLICATE,
                editing::EditorShortcut::Rename => RENAME,
                editing::EditorShortcut::Undo => UNDO,
                editing::EditorShortcut::Redo => REDO,
            },
        )
    } else if root == e.json
        && !e.json.is_null()
        && IsWindowEnabled(e.json) != 0
        && command == editing::EditorShortcut::Save
    {
        (e.json, JSON_SAVE)
    } else if command == editing::EditorShortcut::Save
        && (e.aux.as_ref().is_some_and(|a| a.hwnd == root)
            || e.profiles.as_ref().is_some_and(|p| p.hwnd == root)
            || e.tools == Some(root))
    {
        // A modal draft must not save the parent or insert a control character.
        return true;
    } else {
        return false;
    };
    if msg.lParam & (1 << 30) == 0 {
        PostMessageW(window, WM_COMMAND, id as usize, 0);
    }
    true
}
unsafe fn close_aux(e: &mut Editor, focus: bool) {
    if let Some(aux) = e.aux.take() {
        DestroyWindow(aux.hwnd);
    }
    EnableWindow(e.hwnd, 1);
    if focus && IsWindowVisible(e.hwnd) != 0 {
        super::focus_window(e.hwnd);
    }
}
unsafe fn aux_enable(hwnd: HWND, program: bool) {
    for id in [AUX_PATH, AUX_ARGS, AUX_BROWSE] {
        EnableWindow(dlg(hwnd, id), program as i32);
    }
}
unsafe fn open_aux(e: &mut Editor, kind: AuxKind) {
    open_aux_inner(e, kind, true);
}
unsafe fn open_aux_inner(e: &mut Editor, kind: AuxKind, validate: bool) {
    if matches!(kind, AuxKind::Recovery) && !e.has_recovery() {
        e.status = "No earlier recovery draft is pending.".into();
        return;
    }
    if let Some(aux) = &e.aux {
        super::focus_window(aux.hwnd);
        return;
    }
    if validate && matches!(kind, AuxKind::Browser(_)) {
        if let Err(error) = e.apply() {
            e.status = error;
            InvalidateRect(e.hwnd, null(), 0);
            return;
        }
    }
    let class = wide("Constellation.ItemDialog");
    let module = GetModuleHandleW(null());
    let wc = WNDCLASSW {
        hIcon: super::icons::large(),
        lpfnWndProc: Some(aux_proc),
        hInstance: module,
        lpszClassName: class.as_ptr(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        ..zeroed()
    };
    RegisterClassW(&wc);
    let browser = matches!(kind, AuxKind::Browser(_));
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
    let mut bounds = RECT {
        left: 0,
        top: 0,
        right: Dpi(GetDpiForWindow(e.hwnd)).pixels(600),
        bottom: Dpi(GetDpiForWindow(e.hwnd)).pixels(if browser { 330 } else { 190 }),
    };
    AdjustWindowRectExForDpi(&mut bounds, style, 0, 0, GetDpiForWindow(e.hwnd));
    let mut owner: RECT = zeroed();
    GetWindowRect(e.hwnd, &mut owner);
    let monitor = MonitorFromWindow(e.hwnd, MONITOR_DEFAULTTONEAREST);
    let mut info: MONITORINFO = zeroed();
    info.cbSize = size_of::<MONITORINFO>() as u32;
    let work = if GetMonitorInfoW(monitor, &mut info) != 0 {
        info.rcWork
    } else {
        owner
    };
    let as_rect = |r: RECT| FormRect {
        x: r.left,
        y: r.top,
        w: r.right - r.left,
        h: r.bottom - r.top,
    };
    let (x, y) = crate::editor_layout::dialog_position(
        as_rect(owner),
        as_rect(work),
        bounds.right - bounds.left,
        bounds.bottom - bounds.top,
    );
    let hwnd = CreateWindowExW(
        WS_EX_COMPOSITED,
        class.as_ptr(),
        wide(if browser {
            "Constellation — Web browser"
        } else {
            if matches!(kind, AuxKind::Recovery) {
                "Constellation — Recover draft"
            } else {
                "Constellation — Unsaved changes"
            }
        })
        .as_ptr(),
        style,
        x,
        y,
        bounds.right - bounds.left,
        bounds.bottom - bounds.top,
        e.hwnd,
        null_mut(),
        module,
        null(),
    );
    if hwnd.is_null() {
        e.status = "Cannot create the configuration dialog".into();
        return;
    }
    e.aux = Some(Aux { hwnd, kind });
    theme::window(hwnd);
    super::icons::set_small(hwnd);
    if let AuxKind::Browser(id) = kind {
        let root = id == e.c.universes[e.universe].id;
        let node = e.c.find(id).unwrap();
        label(hwnd, &format!("Browser for {}", node.name), 20, 15, 560);
        label(
            hwnd,
            if root {
                "URL Stars inherit this choice unless they override it."
            } else {
                "This choice overrides the universe browser for this URL only."
            },
            20,
            42,
            560,
        );
        control(
            hwnd,
            "COMBOBOX",
            "",
            AUX_MODE,
            20,
            75,
            560,
            180,
            CBS_DROPDOWNLIST as u32 | WS_TABSTOP,
        );
        let choices = if root {
            vec!["System default", "Custom executable"]
        } else {
            vec!["Inherit universe", "System default", "Custom executable"]
        };
        for choice in choices {
            send(
                hwnd,
                AUX_MODE,
                CB_ADDSTRING,
                0,
                wide(choice).as_ptr() as isize,
            );
        }
        let program = if root { 1 } else { 2 };
        let selected = match &node.browser {
            Some(Browser::Program { .. }) => program,
            Some(Browser::System) if !root => 1,
            _ => 0,
        };
        send(hwnd, AUX_MODE, CB_SETCURSEL, selected, 0);
        label(hwnd, "Browser executable", 20, 114, 560);
        control(
            hwnd,
            "EDIT",
            "",
            AUX_PATH,
            20,
            137,
            460,
            28,
            WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        );
        control(
            hwnd,
            "BUTTON",
            "Browse…",
            AUX_BROWSE,
            490,
            137,
            90,
            28,
            WS_TABSTOP,
        );
        label(
            hwnd,
            "Optional arguments — the URL is appended as one argument",
            20,
            177,
            560,
        );
        control(
            hwnd,
            "EDIT",
            "",
            AUX_ARGS,
            20,
            200,
            560,
            28,
            WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        );
        if let Some(Browser::Program { path, args }) = &node.browser {
            set(hwnd, AUX_PATH, path);
            set(hwnd, AUX_ARGS, &editing::format_arguments(args));
        }
        aux_enable(hwnd, selected == program);
        control(
            hwnd,
            "BUTTON",
            "Apply to draft",
            AUX_APPLY,
            20,
            280,
            150,
            32,
            WS_TABSTOP,
        );
        control(
            hwnd, "BUTTON", "Cancel", AUX_CANCEL, 185, 280, 110, 32, WS_TABSTOP,
        );
        control(hwnd, "STATIC", "", AUX_STATUS, 20, 238, 560, 38, 0);
    } else {
        label(
            hwnd,
            if matches!(kind, AuxKind::Recovery) {
                "Recover unfinished configuration edits? Saved settings stay unchanged."
            } else {
                "Save changes before closing configuration?"
            },
            20,
            20,
            560,
        );
        control(hwnd, "STATIC", "", AUX_STATUS, 20, 52, 560, 65, 0);
        control(
            hwnd,
            "BUTTON",
            if matches!(kind, AuxKind::Recovery) {
                "Restore draft"
            } else {
                "Save"
            },
            AUX_APPLY,
            20,
            130,
            150,
            32,
            WS_TABSTOP,
        );
        control(
            hwnd,
            "BUTTON",
            "Discard",
            AUX_DISCARD,
            185,
            130,
            150,
            32,
            WS_TABSTOP,
        );
        control(
            hwnd, "BUTTON", "Cancel", AUX_CANCEL, 350, 130, 150, 32, WS_TABSTOP,
        );
    }
    if matches!(kind, AuxKind::Recovery) {
        if let Some(message) = &e.recovery_error {
            set(
                hwnd,
                AUX_STATUS,
                &format!(
                    "Recovery cannot be read: {message}. The file is preserved until you choose Discard."
                ),
            );
            EnableWindow(dlg(hwnd, AUX_APPLY), 0);
        }
        if let Some(draft) = &e.recovery_draft {
            set(
                hwnd,
                AUX_STATUS,
                if draft.baseline != crate::draft::fingerprint(&e.saved) {
                    "Saved configuration has changed since this draft. Restoring keeps it as an unsaved draft; review before Save."
                } else {
                    "One recovery file was found. Restore it to review, or Discard to remove it."
                },
            );
        }
    }
    EnableWindow(e.hwnd, 0);
    if IsWindowVisible(e.hwnd) != 0 {
        ShowWindow(hwnd, SW_SHOW);
        super::focus_window(hwnd);
        SetFocus(dlg(hwnd, if browser { AUX_MODE } else { AUX_CANCEL }));
    }
}
unsafe extern "system" fn aux_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(result) = theme::display_message(hwnd, msg, w, l) {
        return result;
    }
    if msg == WM_COMMAND && !EDITOR.is_null() {
        SetTimer((*EDITOR).hwnd, DRAFT_TIMER, 500, None);
    }
    if let Some(result) = theme::message(msg, w, l) {
        return result;
    }
    if EDITOR.is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let Some(guard) = super::DispatchGuard::enter() else {
        if super::defer_nested_close(hwnd, msg, w, l) {
            return 0;
        }
        if msg == WM_COMMAND {
            super::defer_message(hwnd, msg, w, l);
            return 0;
        }
        return DefWindowProcW(hwnd, msg, w, l);
    };
    // A file picker runs its own native loop. Retain no editor borrow/guard.
    if msg == WM_COMMAND && w & 0xffff == AUX_BROWSE as usize {
        let root = (*EDITOR).root.clone();
        drop(guard);
        let choice = pickers::choose(hwnd, &root, Pick::Runner);
        if IsWindow(hwnd) != 0 {
            match choice {
                Ok(Some(path)) => set(hwnd, AUX_PATH, &editing::portable_path(&root, &path)),
                Err(error) => set(hwnd, AUX_STATUS, &error),
                _ => {}
            }
        }
        return 0;
    }
    let e = &mut *EDITOR;
    let Some(aux) = &e.aux else {
        return DefWindowProcW(hwnd, msg, w, l);
    };
    if aux.hwnd != hwnd {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let kind = aux.kind;
    match msg {
        WM_PAINT => {
            render::buffered(hwnd, theme::background(), |_| {});
            0
        }
        WM_COMMAND => {
            let id = (w & 0xffff) as i32;
            if id == AUX_MODE && w >> 16 == CBN_SELCHANGE as usize {
                if let AuxKind::Browser(node) = kind {
                    aux_enable(
                        hwnd,
                        send(hwnd, AUX_MODE, CB_GETCURSEL, 0, 0)
                            == if node == e.c.universes[e.universe].id {
                                1
                            } else {
                                2
                            },
                    );
                }
                return 0;
            }
            match id {
                AUX_CANCEL | 2 => {
                    close_aux(e, true);
                }
                AUX_DISCARD if matches!(kind, AuxKind::Recovery) => {
                    let cleared = if let Some(writer) = &e.draft_writer {
                        writer.clear()
                    } else {
                        Err("Recovery writer unavailable".into())
                    };
                    if let Err(message) = cleared {
                        set(hwnd, AUX_STATUS, &message);
                        return 0;
                    }
                    e.recovery_draft = None;
                    e.recovery_error = None;
                    close_aux(e, true);
                }
                AUX_DISCARD if matches!(kind, AuxKind::Close) => {
                    if !e.has_recovery() {
                        if let Some(writer) = &e.draft_writer {
                            let _ = writer.clear();
                        }
                    }
                    close_aux(e, false);
                    DestroyWindow(e.hwnd);
                }
                AUX_APPLY | 1 => {
                    let result = (|| -> Result<(), String> {
                        if matches!(kind, AuxKind::Recovery) {
                            close_aux(e, true);
                            e.restore_draft()?;
                        } else if let AuxKind::Browser(id) = kind {
                            let root = id == e.c.universes[e.universe].id;
                            let index = send(hwnd, AUX_MODE, CB_GETCURSEL, 0, 0);
                            let browser = if index == if root { 1 } else { 2 } {
                                Some(Browser::Program {
                                    path: get(hwnd, AUX_PATH),
                                    args: editing::parse_arguments(&get(hwnd, AUX_ARGS))?,
                                })
                            } else if !root && index == 1 {
                                Some(Browser::System)
                            } else {
                                None
                            };
                            let mut c = e.c.clone();
                            c.universes[e.universe]
                                .find_mut(id)
                                .ok_or("Item no longer exists")?
                                .browser = browser;
                            c.validate()?;
                            e.commit(c);
                            close_aux(e, true);
                            e.refresh();
                            e.status = "Browser updated in the draft. Save to apply it.".into();
                            InvalidateRect(e.hwnd, null(), 0);
                        } else {
                            e.save()?;
                            close_aux(e, false);
                            DestroyWindow(e.hwnd);
                        }
                        Ok(())
                    })();
                    if let Err(error) = result {
                        set(hwnd, AUX_STATUS, &error);
                    }
                }
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            close_aux(e, true);
            0
        }
        _ => {
            drop(guard);
            DefWindowProcW(hwnd, msg, w, l)
        }
    }
}

// Small, owned tool windows keep this editor's draft/undo/save authority.
unsafe fn show_tools_menu(hwnd: HWND) -> LRESULT {
    let Some(guard) = super::DispatchGuard::enter() else {
        super::defer_message(hwnd, WM_COMMAND, TOOLS as usize, 0);
        return 0;
    };
    let menu = CreatePopupMenu();
    for (id, name) in [
        (super::editor_tools::THEME, "Themes & colors..."),
        (super::editor_tools::MOVE, "Move items..."),
        (super::editor_tools::PATHS, "Check / repair paths..."),
        (RECOVER_DRAFT as u32, "Recover draft..."),
    ] {
        let disabled =
            id == RECOVER_DRAFT as u32 && (EDITOR.is_null() || !(*EDITOR).has_recovery());
        AppendMenuW(
            menu,
            MF_STRING | if disabled { MF_GRAYED } else { 0 },
            id as usize,
            wide(name).as_ptr(),
        );
    }
    let mut rect: RECT = zeroed();
    GetWindowRect(dlg(hwnd, TOOLS), &mut rect);
    drop(guard);
    let id = TrackPopupMenuEx(
        menu,
        TPM_RETURNCMD | TPM_NONOTIFY,
        rect.left,
        rect.bottom,
        hwnd,
        null(),
    ) as u32;
    DestroyMenu(menu);
    if id != 0 {
        PostMessageW(hwnd, WM_COMMAND, id as usize, 0);
    }
    0
}
impl Editor {
    unsafe fn open_tool(&mut self, kind: u32) -> Result<(), String> {
        if let Some(hwnd) = self.tools {
            super::focus_window(hwnd);
            return Ok(());
        }
        self.apply()?;
        self.controls_to_settings()?;
        self.checkpoint_settings();
        self.tools = Some(super::editor_tools::open(
            self.hwnd,
            self.root.clone(),
            self.c.clone(),
            self.universe,
            self.selected,
            kind,
        )?);
        Ok(())
    }
    unsafe fn finish_tool(&mut self, hwnd: HWND) {
        if self.tools != Some(hwnd) {
            return;
        }
        let Some(action) = super::editor_tools::take_result(hwnd) else {
            return;
        };
        let result = (|| -> Result<(), String> {
            let mut candidate = self.c.clone();
            let mut selected = self.selected;
            let status = match action {
                super::editor_tools::Action::Theme(palette) => {
                    candidate.theme = palette;
                    "Palette updated in this draft. Save applies it to the launcher.".to_string()
                }
                super::editor_tools::Action::Move(ids, destination) => {
                    let count = crate::organize::move_nodes(&mut candidate, &ids, destination)?;
                    selected = *ids.first().unwrap();
                    format!("Moved {count} items. Undo restores the move; Save persists it.")
                }
                super::editor_tools::Action::Relink(old, new) => {
                    let count = crate::organize::relink(&mut candidate, self.universe, &old, &new)?;
                    format!(
                        "Replaced {count} reviewed path fields in the draft. Undo restores them; Save persists them."
                    )
                }
            };
            candidate.validate()?;
            self.commit(candidate);
            self.universe = self
                .c
                .universes
                .iter()
                .position(|u| u.find(selected).is_some())
                .unwrap_or(self.universe);
            self.selected = selected;
            self.tools = None;
            DestroyWindow(hwnd);
            EnableWindow(self.hwnd, 1);
            super::focus_window(self.hwnd);
            theme::refresh(self.hwnd, self.c.theme);
            self.preview_cache = None;
            self.refresh();
            self.settings_to_controls();
            self.status = status;
            SetTimer(self.hwnd, DRAFT_TIMER, 500, None);
            Ok(())
        })();
        if let Err(error) = result {
            super::editor_tools::error(hwnd, &error);
        }
    }
    unsafe fn capture_draft(&self) {
        if self.has_recovery() {
            return;
        }
        let Some(writer) = &self.draft_writer else {
            return;
        };
        if !(self.dirty
            || self.item_dirty
            || self.settings_invalid
            || self.profiles.is_some()
            || !self.json.is_null()
            || self.tools.is_some()
            || self
                .aux
                .as_ref()
                .is_some_and(|a| matches!(a.kind, AuxKind::Browser(_))))
        {
            let _ = writer.clear();
            return;
        }
        let mut fields = std::collections::BTreeMap::new();
        for id in [
            NAME, NOTE, TARGET, ARGS, RUNNER, CWD, RADIUS, DISTANCE, SPREAD, GAP, VOLUME,
        ] {
            fields.insert(format!("main:{id}"), get(self.hwnd, id));
        }
        for id in [KIND, PARENT, ICON_MODE, INSTRUMENT] {
            fields.insert(
                format!("combo:{id}"),
                send(self.hwnd, id, CB_GETCURSEL, 0, 0).to_string(),
            );
        }
        let visible: Vec<_> = self
            .audience()
            .iter()
            .enumerate()
            .filter(|(i, _)| send(self.hwnd, VISIBLE, LB_GETSEL, *i, 0) > 0)
            .map(|(_, p)| p.0)
            .collect();
        fields.insert("visible".into(), serde_json::to_string(&visible).unwrap());
        fields.insert(
            "icon".into(),
            serde_json::to_string(&self.icon_draft).unwrap(),
        );
        if !self.json.is_null() {
            fields.insert("json".into(), get(self.json, JSON_TEXT));
        }
        if let Some(p) = &self.profiles {
            fields.insert("profiles".into(), serde_json::to_string(&p.draft).unwrap());
            fields.insert("profile_index".into(), p.selected.unwrap_or(0).to_string());
            for id in [P_NAME, P_BINDING] {
                fields.insert(format!("profile_text:{id}"), get(p.hwnd, id));
            }
            for id in [P_MODE, P_RESTORE, P_UNIVERSE, P_ENTRY] {
                fields.insert(
                    format!("profile_combo:{id}"),
                    send(p.hwnd, id, CB_GETCURSEL, 0, 0).to_string(),
                );
            }
            for id in [P_HOVER, P_DIRECTIONAL, P_DOT] {
                fields.insert(
                    format!("profile_check:{id}"),
                    send(p.hwnd, id, BM_GETCHECK, 0, 0).to_string(),
                );
            }
        }
        if let Some(aux) = &self.aux {
            if let AuxKind::Browser(id) = aux.kind {
                fields.insert("browser_node".into(), id.to_string());
                fields.insert(
                    "browser_mode".into(),
                    send(aux.hwnd, AUX_MODE, CB_GETCURSEL, 0, 0).to_string(),
                );
                fields.insert("browser_path".into(), get(aux.hwnd, AUX_PATH));
                fields.insert("browser_args".into(), get(aux.hwnd, AUX_ARGS));
            }
        }
        if let Some(hwnd) = self.tools {
            if let Some(value) = super::editor_tools::capture(hwnd) {
                fields.insert("tools".into(), value);
            }
        }
        writer.submit(crate::draft::Draft {
            version: 1,
            baseline: crate::draft::fingerprint(&self.saved),
            config: self.c.clone(),
            universe: self.c.universes[self.universe].id,
            selected: self.selected,
            fields,
        });
    }
    unsafe fn restore_draft(&mut self) -> Result<(), String> {
        let draft = self.recovery_draft.clone().ok_or("No recovery draft")?;
        let baseline_changed = draft.baseline != crate::draft::fingerprint(&self.saved);
        self.commit(draft.config.clone());
        self.universe = self
            .c
            .universes
            .iter()
            .position(|u| u.id == draft.universe)
            .unwrap_or(0);
        self.selected = if self.c.universes[self.universe]
            .find(draft.selected)
            .is_some()
        {
            draft.selected
        } else {
            self.c.universes[self.universe].id
        };
        theme::refresh(self.hwnd, self.c.theme);
        self.refresh();
        self.settings_to_controls();
        for id in [
            NAME, NOTE, TARGET, ARGS, RUNNER, CWD, RADIUS, DISTANCE, SPREAD, GAP, VOLUME,
        ] {
            if let Some(value) = draft.fields.get(&format!("main:{id}")) {
                set(self.hwnd, id, value);
            }
        }
        for id in [KIND, PARENT, ICON_MODE, INSTRUMENT] {
            if let Some(index) = draft
                .fields
                .get(&format!("combo:{id}"))
                .and_then(|v| v.parse::<isize>().ok())
            {
                if index >= 0 && index < send(self.hwnd, id, CB_GETCOUNT, 0, 0) {
                    send(self.hwnd, id, CB_SETCURSEL, index as usize, 0);
                }
            }
        }
        if let Some(icon) = draft
            .fields
            .get("icon")
            .and_then(|v| serde_json::from_str(v).ok())
        {
            self.icon_draft = icon;
        }
        if let Some(visible) = draft
            .fields
            .get("visible")
            .and_then(|v| serde_json::from_str::<Vec<u64>>(v).ok())
        {
            send(self.hwnd, VISIBLE, LB_SETSEL, 0, -1);
            for (i, p) in self.audience().iter().enumerate() {
                send(
                    self.hwnd,
                    VISIBLE,
                    LB_SETSEL,
                    visible.contains(&p.0) as usize,
                    i as isize,
                );
            }
        }
        self.enable_fields();
        self.note_counter();
        self.item_dirty = true;
        self.settings_invalid = self.controls_to_settings().is_err();
        self.dirty = true;
        if let Some(json) = draft.fields.get("json") {
            advanced(self);
            if !self.json.is_null() {
                set(self.json, JSON_TEXT, json);
            }
        }
        if let Some(list) = draft
            .fields
            .get("profiles")
            .and_then(|v| serde_json::from_str::<Vec<Profile>>(v).ok())
        {
            profiles_inner(self, false);
            if let Some(p) = &mut self.profiles {
                p.draft = list;
                p.selected = draft
                    .fields
                    .get("profile_index")
                    .and_then(|v| v.parse::<usize>().ok())
                    .filter(|i| *i < p.draft.len());
                p.refresh();
                for id in [P_NAME, P_BINDING] {
                    if let Some(value) = draft.fields.get(&format!("profile_text:{id}")) {
                        set(p.hwnd, id, value);
                    }
                }
                for id in [P_MODE, P_RESTORE, P_UNIVERSE] {
                    if let Some(index) = draft
                        .fields
                        .get(&format!("profile_combo:{id}"))
                        .and_then(|v| v.parse::<usize>().ok())
                    {
                        send(p.hwnd, id, CB_SETCURSEL, index, 0);
                    }
                }
                let tree_index = send(p.hwnd, P_UNIVERSE, CB_GETCURSEL, 0, 0).max(0) as usize;
                fill_entries(p.hwnd, p.trees.get(tree_index), None);
                if let Some(index) = draft
                    .fields
                    .get(&format!("profile_combo:{P_ENTRY}"))
                    .and_then(|v| v.parse::<usize>().ok())
                {
                    send(p.hwnd, P_ENTRY, CB_SETCURSEL, index, 0);
                }
                for id in [P_HOVER, P_DIRECTIONAL, P_DOT] {
                    if let Some(value) = draft
                        .fields
                        .get(&format!("profile_check:{id}"))
                        .and_then(|v| v.parse::<usize>().ok())
                    {
                        send(p.hwnd, id, BM_SETCHECK, usize::from(value != 0), 0);
                    }
                }
            }
        } else if let Some(id) = draft
            .fields
            .get("browser_node")
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|id| self.c.find(*id).is_some())
        {
            open_aux_inner(self, AuxKind::Browser(id), false);
            if let Some(aux) = &self.aux {
                let hwnd = aux.hwnd;
                for (key, id) in [("browser_path", AUX_PATH), ("browser_args", AUX_ARGS)] {
                    if let Some(value) = draft.fields.get(key) {
                        set(hwnd, id, value);
                    }
                }
                if let Some(index) = draft
                    .fields
                    .get("browser_mode")
                    .and_then(|v| v.parse::<usize>().ok())
                {
                    send(hwnd, AUX_MODE, CB_SETCURSEL, index, 0);
                    aux_enable(
                        hwnd,
                        index
                            == if id == self.c.universes[self.universe].id {
                                1
                            } else {
                                2
                            },
                    );
                }
            }
        } else if let Some(value) = draft.fields.get("tools") {
            if let Some(kind) = super::editor_tools::recovery_kind(value) {
                self.tools = Some(super::editor_tools::open(
                    self.hwnd,
                    self.root.clone(),
                    self.c.clone(),
                    self.universe,
                    self.selected,
                    kind,
                )?);
                if let Some(hwnd) = self.tools {
                    super::editor_tools::restore(hwnd, value);
                }
            }
        }
        self.recovery_draft = None;
        self.recovery_error = None;
        self.status=if baseline_changed{"Recovered as an unsaved draft. The saved configuration changed since the recovery point; review before Save."}else{"Recovered unfinished edits into this draft. Save applies them to the launcher."}.into();
        InvalidateRect(self.hwnd, null(), 0);
        SetTimer(self.hwnd, DRAFT_TIMER, 500, None);
        Ok(())
    }
}
