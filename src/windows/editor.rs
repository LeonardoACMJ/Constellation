#![allow(unsafe_op_in_unsafe_fn)]
use super::{
    editor_theme as theme, error,
    pickers::{self, Pick},
    render, wide,
};
use crate::{
    config::{Binding, Config, CursorEnd, Mode, Node, Profile, Target},
    editing::{self, History},
    geometry::{self, Point},
    storage,
};
use std::{
    mem::{size_of, zeroed},
    path::PathBuf,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{Com::*, LibraryLoader::GetModuleHandleW},
    UI::{Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

const TREE: i32 = 100;
const BROWSE_TARGET: i32 = 211;
const BROWSE_RUNNER: i32 = 212;
const BROWSE_CWD: i32 = 213;
const ADVICE: i32 = 214;
const UNDO: i32 = 405;
const REDO: i32 = 406;
const IMPORT: i32 = 407;
const EXPORT: i32 = 408;
const NAME: i32 = 201;
const KIND: i32 = 202;
const TARGET: i32 = 203;
const ARGS: i32 = 204;
const RUNNER: i32 = 205;
const CWD: i32 = 206;
const PARENT: i32 = 207;
const APPLY: i32 = 301;
const ADD_STAR: i32 = 302;
const ADD_FOLDER: i32 = 303;
const DELETE: i32 = 304;
const UP: i32 = 305;
const DOWN: i32 = 306;
const SAVE: i32 = 401;
const RELOAD: i32 = 402;
const ADVANCED: i32 = 403;
const PROFILES: i32 = 404;
const JSON_TEXT: i32 = 410;
const JSON_SAVE: i32 = 411;
const RADIUS: i32 = 501;
const DISTANCE: i32 = 502;
const SPREAD: i32 = 503;
const GAP: i32 = 504;
const SOUND: i32 = 505;
const VOLUME: i32 = 506;
const MONITOR: i32 = 507;
struct Display {
    handle: HMONITOR,
    work: RECT,
}
struct Editor {
    hwnd: HWND,
    root: PathBuf,
    c: Config,
    rows: Vec<u64>,
    parents: Vec<u64>,
    selected: u64,
    displays: Vec<Display>,
    monitor: usize,
    json: HWND,
    profiles: Option<Profiles>,
    status: String,
    preview: RECT,
    dirty: bool,
    scroll_x: i32,
    scroll_y: i32,
    saved: Config,
    history: History,
    settings_before: Option<Config>,
    item_dirty: bool,
    settings_invalid: bool,
    measure_preview: bool,
    preview_cache: Option<PreviewCache>,
    wheel_delta: i32,
}
struct PreviewCache {
    config: Config,
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
    let hwnd = CreateWindowExW(
        0,
        wide(class).as_ptr(),
        wide(label).as_ptr(),
        WS_CHILD | WS_VISIBLE | style | if class == "STATIC" { 0 } else { WS_TABSTOP },
        x,
        y,
        w,
        h,
        hwnd,
        id as usize as HMENU,
        GetModuleHandleW(null()),
        null(),
    );
    theme::control(hwnd, class);
    hwnd
}
unsafe fn get(hwnd: HWND, id: i32) -> String {
    let e = GetDlgItem(hwnd, id);
    let len = GetWindowTextLengthW(e);
    let mut b = vec![0u16; len as usize + 1];
    GetWindowTextW(e, b.as_mut_ptr(), b.len() as i32);
    String::from_utf16_lossy(&b[..len as usize])
}
unsafe fn set(hwnd: HWND, id: i32, s: &str) {
    SetWindowTextW(GetDlgItem(hwnd, id), wide(s).as_ptr());
}
unsafe fn send(hwnd: HWND, id: i32, msg: u32, w: usize, l: isize) -> isize {
    SendMessageW(GetDlgItem(hwnd, id), msg, w, l)
}
unsafe fn label(hwnd: HWND, text: &str, x: i32, y: i32, w: i32) {
    control(hwnd, "STATIC", text, 0, x, y, w, 18, 0);
}

pub fn run(root: PathBuf) -> Result<(), String> {
    unsafe {
        let controls = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_BAR_CLASSES,
        };
        if InitCommonControlsEx(&controls) == 0 {
            return Err("Cannot initialize layout sliders".into());
        }
        if CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) < 0 {
            return Err("Cannot initialize folder picker".into());
        }
        let c = storage::load_or_create(&root)?;
        theme::install();
        let module = GetModuleHandleW(null());
        let class = wide("Constellation.Config");
        let wc = WNDCLASSW {
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
            wide("Constellation — Configure & Visualize").as_ptr(),
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
        if hwnd.is_null() {
            return Err("Cannot create configuration window".into());
        }
        theme::window(hwnd);
        let mut displays = vec![];
        EnumDisplayMonitors(
            null_mut(),
            null(),
            Some(monitor_callback),
            &mut displays as *mut Vec<Display> as isize,
        );
        let selected = c.root.id;
        EDITOR = Box::into_raw(Box::new(Editor {
            hwnd,
            root,
            saved: c.clone(),
            history: History::default(),
            settings_before: None,
            item_dirty: false,
            settings_invalid: false,
            measure_preview: std::env::args().any(|s| s == "--measure-preview"),
            preview_cache: None,
            wheel_delta: 0,
            c,
            rows: vec![],
            parents: vec![],
            selected,
            displays,
            monitor: 0,
            json: null_mut(),
            profiles: None,
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
        }));
        let startup_guard = super::DispatchGuard::enter().expect("editor startup dispatch");
        label(hwnd, "STARS & CONSTELLATIONS", 20, 18, 270);
        control(
            hwnd,
            "LISTBOX",
            "",
            TREE,
            20,
            45,
            260,
            470,
            WS_BORDER | WS_VSCROLL | LBS_NOTIFY as u32,
        );
        for (id, title, x, y, w) in [
            (ADD_STAR, "+ Star", 20, 525, 80),
            (ADD_FOLDER, "+ Constellation", 110, 525, 170),
            (DELETE, "Delete", 20, 565, 80),
            (UP, "Up", 110, 565, 80),
            (DOWN, "Down", 200, 565, 80),
        ] {
            control(hwnd, "BUTTON", title, id, x, y, w, 30, 0);
        }
        label(hwnd, "ITEM", 305, 18, 340);
        for (id, title, y) in [
            (NAME, "Name", 45),
            (KIND, "Kind", 108),
            (TARGET, "Target path / URL", 171),
            (ARGS, "Arguments (quote values containing spaces)", 234),
            (RUNNER, "Script runner (blank = extension default)", 297),
            (CWD, "Working directory (blank = portable folder)", 360),
            (PARENT, "Parent constellation", 423),
        ] {
            label(hwnd, title, 305, y, 360);
            let combo = matches!(id, KIND | PARENT);
            control(
                hwnd,
                if combo { "COMBOBOX" } else { "EDIT" },
                "",
                id,
                305,
                y + 21,
                if matches!(id, TARGET | RUNNER | CWD) {
                    280
                } else {
                    360
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
            (BROWSE_RUNNER, 318),
            (BROWSE_CWD, 381),
        ] {
            control(hwnd, "BUTTON", "Browse…", id, 592, y, 73, 28, 0);
        }
        control(hwnd, "STATIC", "", ADVICE, 305, 528, 370, 40, 0);
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
            55,
            0,
        );
        control(hwnd, "BUTTON", "Apply item", APPLY, 305, 490, 150, 32, 0);
        label(
            hwnd,
            "LAYOUT (numeric values; no forced fitting)",
            305,
            575,
            370,
        );
        for (id, title, x, y) in [
            (RADIUS, "Radius", 305, 605),
            (DISTANCE, "Distance × radius", 490, 605),
            (SPREAD, "Spread °", 305, 665),
            (GAP, "Gap", 490, 665),
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
        control(
            hwnd,
            "BUTTON",
            "Chimes",
            SOUND,
            305,
            735,
            110,
            28,
            BS_AUTOCHECKBOX as u32,
        );
        label(hwnd, "Volume (0–1)", 430, 735, 110);
        control(
            hwnd,
            "EDIT",
            "",
            VOLUME,
            550,
            732,
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
            805,
            100,
            35,
            BS_DEFPUSHBUTTON as u32,
        );
        control(hwnd, "BUTTON", "Reload", RELOAD, 130, 805, 100, 35, 0);
        control(
            hwnd,
            "BUTTON",
            "Shortcut profiles…",
            PROFILES,
            240,
            805,
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
            805,
            170,
            35,
            WS_TABSTOP,
        );
        e.refresh();
        e.settings_to_controls();
        e.dirty = false;
        e.resize_view();
        DragAcceptFiles(hwnd, 1);
        drop(startup_guard);
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        super::focus_window(hwnd);
        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            let e = &*EDITOR;
            let dialog = e.profiles.as_ref().map_or(hwnd, |p| p.hwnd);
            let recording = e.profiles.as_ref().is_some_and(|p| p.recording);
            if recording || IsDialogMessageW(dialog, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        drop(Box::from_raw(EDITOR));
        EDITOR = null_mut();
        theme::shutdown();
        CoUninitialize();
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
    unsafe fn paint_preview(&self, dc: HDC) -> usize {
        let e = self;
        render::fill(dc, &e.preview, render::rgb(15, 22, 36));
        let all = geometry::full(&e.c);
        let (workw, workh) = if let Some(d) = e.displays.get(e.monitor) {
            let (mut x, mut y) = (96, 96);
            GetDpiForMonitor(d.handle, MDT_EFFECTIVE_DPI, &mut x, &mut y);
            (
                (d.work.right - d.work.left) as f64 * 96. / x.max(1) as f64,
                (d.work.bottom - d.work.top) as f64 * 96. / y.max(1) as f64,
            )
        } else {
            (1920., 1080.)
        };
        let mut extentx = workw / 2.;
        let mut extenty = workh / 2.;
        for s in &all {
            for p in s.polygon() {
                extentx = extentx.max(p.x.abs());
                extenty = extenty.max(p.y.abs());
            }
        }
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
        render::draw(dc, &e.c, &all, &[], None, origin, scale, true);
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
        let warnings = geometry::warnings(&e.c, workw, workh);
        let warning = if warnings.is_empty() {
            "Layout fits this monitor's work area.".into()
        } else {
            warnings.join(" ")
        };
        render::text(
            dc,
            &warning,
            r.left,
            r.bottom + 8,
            r.right - r.left,
            theme::MUTED,
            12,
        );
        all.len()
    }
    unsafe fn cached_preview(&mut self, dc: HDC) -> usize {
        let r = self.preview;
        let width = (r.right - r.left).max(1);
        let height = (r.bottom - r.top + 110).max(1);
        let valid = self.preview_cache.as_ref().is_some_and(|v| {
            v.config == self.c
                && v.monitor == self.monitor
                && v.width == width
                && v.height == height
        });
        if !valid {
            let memory = CreateCompatibleDC(dc);
            let bitmap = CreateCompatibleBitmap(dc, width, height);
            let old = SelectObject(memory, bitmap);
            render::fill(
                memory,
                &RECT {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                },
                theme::BACKGROUND,
            );
            SetViewportOrgEx(memory, -r.left, -r.top, null_mut());
            let sectors = self.paint_preview(memory);
            SelectObject(memory, old);
            DeleteDC(memory);
            self.preview_cache = Some(PreviewCache {
                config: self.c.clone(),
                monitor: self.monitor,
                width,
                height,
                bitmap,
                sectors,
            });
        }
        let cache = self.preview_cache.as_ref().unwrap();
        let memory = CreateCompatibleDC(dc);
        let old = SelectObject(memory, cache.bitmap);
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
    unsafe fn advice(&self) {
        let warning = self
            .c
            .find(self.selected)
            .and_then(|n| n.target.as_ref())
            .map(|t| editing::target_advice(&self.root, t).join(" "))
            .unwrap_or_default();
        set(self.hwnd, ADVICE, &warning);
    }
    unsafe fn enable_fields(&self) {
        let kind = send(self.hwnd, KIND, CB_GETCURSEL, 0, 0);
        for (id, on) in [
            (TARGET, kind != 0),
            (ARGS, matches!(kind, 1 | 5)),
            (RUNNER, kind == 5),
            (CWD, matches!(kind, 1 | 5)),
            (BROWSE_TARGET, matches!(kind, 1 | 2 | 4 | 5)),
            (BROWSE_RUNNER, kind == 5),
            (BROWSE_CWD, matches!(kind, 1 | 5)),
        ] {
            EnableWindow(GetDlgItem(self.hwnd, id), on as i32);
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
            find_parent(&self.c.root, self.selected).unwrap_or(self.c.root.id)
        };
        let mut candidate = self.c.clone();
        let mut id = candidate.root.max_id();
        let mut added = vec![];
        for path in paths {
            id = id.checked_add(1).ok_or("Item ID space exhausted")?;
            added.push(editing::star_from_path(&self.root, &path, id)?);
        }
        if added.is_empty() {
            return Ok(());
        }
        let count = added.len();
        candidate
            .root
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
                editing::export(&path, &self.c)?;
                self.status =
                    "Exported configuration. Launch targets and assets are not copied.".into();
            } else {
                let c = editing::read_import(&path)?;
                self.commit(c);
                self.selected = self.c.root.id;
                self.refresh();
                self.settings_to_controls();
                self.status="Imported into the draft. Undo restores the previous tree; Save applies the import.".into();
            }
        }
        Ok(())
    }
    unsafe fn scroll_to(&mut self, x: i32, y: i32) {
        let (dx, dy) = (self.scroll_x - x, self.scroll_y - y);
        if dx == 0 && dy == 0 {
            return;
        }
        self.scroll_x = x;
        self.scroll_y = y;
        SetScrollPos(self.hwnd, SB_HORZ, x, 0);
        SetScrollPos(self.hwnd, SB_VERT, y, 0);
        let mut positions = vec![];
        let mut child = GetWindow(self.hwnd, GW_CHILD);
        while !child.is_null() {
            let mut r: RECT = zeroed();
            GetWindowRect(child, &mut r);
            let mut p = POINT {
                x: r.left,
                y: r.top,
            };
            ScreenToClient(self.hwnd, &mut p);
            positions.push((child, p.x + dx, p.y + dy));
            child = GetWindow(child, GW_HWNDNEXT);
        }
        let flags = SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOREDRAW;
        let mut batch = BeginDeferWindowPos(positions.len() as i32);
        for &(child, x, y) in &positions {
            if batch.is_null() {
                break;
            }
            batch = DeferWindowPos(batch, child, null_mut(), x, y, 0, 0, flags);
        }
        let moved = !batch.is_null() && EndDeferWindowPos(batch) != 0;
        if !moved {
            for (child, x, y) in positions {
                SetWindowPos(child, null_mut(), x, y, 0, 0, flags);
            }
        }
        // One combined invalidation, no intermediate erase or per-child paint.
        RedrawWindow(
            self.hwnd,
            null(),
            null_mut(),
            RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_NOERASE,
        );
    }
    unsafe fn resize_view(&mut self) {
        let mut client: RECT = zeroed();
        GetClientRect(self.hwnd, &mut client);
        let mut positions = [0; 2];
        for (i, bar, page, max, pos) in [
            (
                0,
                SB_HORZ,
                client.right,
                client.right.max(1050),
                self.scroll_x,
            ),
            (
                1,
                SB_VERT,
                client.bottom,
                client.bottom.max(895),
                self.scroll_y,
            ),
        ] {
            let info = SCROLLINFO {
                cbSize: size_of::<SCROLLINFO>() as u32,
                fMask: SIF_RANGE | SIF_PAGE | SIF_POS | SIF_DISABLENOSCROLL,
                nMin: 0,
                nMax: max - 1,
                nPage: page.max(1) as u32,
                nPos: pos,
                nTrackPos: 0,
            };
            positions[i] = SetScrollInfo(self.hwnd, bar, &info, 1);
        }
        self.scroll_to(positions[0], positions[1]);
        self.preview.right = client.right.max(1050) - 30;
        MoveWindow(
            GetDlgItem(self.hwnd, MONITOR),
            self.preview.left - self.scroll_x,
            40 - self.scroll_y,
            self.preview.right - self.preview.left,
            250,
            1,
        );
        InvalidateRect(self.hwnd, null(), 1);
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

    unsafe fn refresh(&mut self) {
        send(self.hwnd, TREE, LB_RESETCONTENT, 0, 0);
        self.rows.clear();
        self.parents.clear();
        send(self.hwnd, PARENT, CB_RESETCONTENT, 0, 0);
        fn walk(
            n: &Node,
            depth: usize,
            rows: &mut Vec<(u64, String)>,
            parents: &mut Vec<(u64, String)>,
        ) {
            let text = format!(
                "{}{} {}",
                "  ".repeat(depth),
                if n.children.is_some() { "✧" } else { "✦" },
                n.name
            );
            rows.push((n.id, text.clone()));
            if let Some(cs) = &n.children {
                parents.push((n.id, text));
                for c in cs {
                    walk(c, depth + 1, rows, parents);
                }
            }
        }
        let mut rows = vec![];
        let mut parents = vec![];
        walk(&self.c.root, 0, &mut rows, &mut parents);
        for (id, text) in rows {
            self.rows.push(id);
            send(
                self.hwnd,
                TREE,
                LB_ADDSTRING,
                0,
                wide(text).as_ptr() as isize,
            );
        }
        for (id, text) in parents {
            self.parents.push(id);
            send(
                self.hwnd,
                PARENT,
                CB_ADDSTRING,
                0,
                wide(text).as_ptr() as isize,
            );
        }
        let index = self
            .rows
            .iter()
            .position(|id| *id == self.selected)
            .unwrap_or(0);
        self.selected = self.rows[index];
        send(self.hwnd, TREE, LB_SETCURSEL, index, 0);
        self.selection_to_controls();
        InvalidateRect(self.hwnd, null(), 1);
    }
    unsafe fn selection_to_controls(&mut self) {
        let Some(n) = self.c.find(self.selected) else {
            return;
        };
        set(self.hwnd, NAME, &n.name);
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
        let parent = find_parent(&self.c.root, self.selected).unwrap_or(self.c.root.id);
        let idx = self
            .parents
            .iter()
            .position(|id| *id == parent)
            .unwrap_or(0);
        send(self.hwnd, PARENT, CB_SETCURSEL, idx, 0);
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
            send(
                self.hwnd,
                id + 10,
                TBM_SETPOS,
                1,
                value.round().clamp(0., i32::MAX as f64) as isize,
            );
        }
    }
    unsafe fn settings_to_controls(&mut self) {
        self.settings_invalid = false;
        self.sync_sliders();
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
        send(
            self.hwnd,
            SOUND,
            BM_SETCHECK,
            usize::from(self.c.sound.enabled),
            0,
        );
    }
    unsafe fn controls_to_settings(&mut self) -> Result<(), String> {
        let number = |id| -> Result<f64, String> {
            get(self.hwnd, id)
                .trim()
                .parse()
                .map_err(|_| "Enter numeric layout and volume values".into())
        };
        let mut candidate = self.c.clone();
        candidate.layout.radius = number(RADIUS)?;
        candidate.layout.distance = number(DISTANCE)?;
        candidate.layout.spread = number(SPREAD)?;
        candidate.layout.gap = number(GAP)?;
        candidate.sound.volume = number(VOLUME)?;
        candidate.sound.enabled = send(self.hwnd, SOUND, BM_GETCHECK, 0, 0) == 1;
        candidate.validate()?;
        if candidate != self.c {
            if self.settings_before.is_none() {
                self.settings_before = Some(self.c.clone());
            }
            self.c = candidate;
            self.dirty = self.c != self.saved;
        }
        self.settings_invalid = false;
        Ok(())
    }
    unsafe fn apply(&mut self) -> Result<(), String> {
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
        let n = candidate.root.find_mut(self.selected).unwrap();
        n.name = get(self.hwnd, NAME);
        n.children = if target.is_none() {
            Some(old.children.clone().unwrap_or_default())
        } else {
            None
        };
        n.target = target;
        if self.selected != candidate.root.id {
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
            if find_parent(&candidate.root, self.selected) != Some(parent) {
                let node =
                    remove(&mut candidate.root, self.selected).ok_or("Cannot move this item")?;
                candidate
                    .root
                    .find_mut(parent)
                    .and_then(|p| p.children.as_mut())
                    .ok_or("Parent is not a constellation")?
                    .push(node);
            }
        }
        candidate.validate()?;
        self.commit(candidate);
        self.item_dirty = false;
        self.refresh();
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
            find_parent(&self.c.root, self.selected).unwrap_or(self.c.root.id)
        };
        let id = self
            .c
            .root
            .max_id()
            .checked_add(1)
            .ok_or("Item ID space exhausted")?;
        let n = Node {
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
        self.c
            .root
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
    unsafe fn reorder(&mut self, down: bool) -> Result<(), String> {
        self.apply()?;
        let before = self.c.clone();
        if let Some(parent) = find_parent(&self.c.root, self.selected) {
            let cs = self
                .c
                .root
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
    if let Some(result) = theme::message(msg, w, l) {
        return result;
    }
    if EDITOR.is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let Some(_guard) = super::DispatchGuard::enter() else {
        if msg == WM_COMMAND && w & 0xffff == JSON_SAVE as usize {
            PostMessageW(hwnd, msg, w, l);
        }
        return DefWindowProcW(hwnd, msg, w, l);
    };
    let e = &mut *EDITOR;
    match msg {
        WM_PAINT => {
            render::buffered(hwnd, theme::BACKGROUND, |_| {});
            0
        }
        WM_SIZE => {
            let width = (l as u32 & 0xffff) as i32;
            let height = ((l as u32 >> 16) & 0xffff) as i32;
            MoveWindow(
                GetDlgItem(hwnd, JSON_TEXT),
                15,
                40,
                (width - 30).max(1),
                (height - 105).max(1),
                1,
            );
            MoveWindow(GetDlgItem(hwnd, JSON_SAVE), 15, height - 50, 170, 32, 1);
            0
        }
        WM_COMMAND if w & 0xffff == JSON_SAVE as usize => {
            let result = (|| -> Result<(), String> {
                let c = storage::parse_config(get(hwnd, JSON_TEXT).as_bytes())?;
                storage::save(&e.root, &c)?;
                notify_resident();
                e.commit(c.clone());
                e.saved = c;
                e.dirty = false;
                e.refresh();
                e.settings_to_controls();
                e.status =
                    "Saved. The resident launcher reloads settings on its next invocation.".into();
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
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(result) = theme::message(msg, w, l) {
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
        if matches!(msg, WM_SIZE | WM_HSCROLL | WM_VSCROLL | WM_DROPFILES) {
            PostMessageW(hwnd, msg, w, l);
        }
        if msg == WM_COMMAND
            && (w >> 16) as u32 == BN_CLICKED
            && (w & 0xffff) >= BROWSE_TARGET as usize
        {
            PostMessageW(hwnd, msg, w, l);
        }
        return DefWindowProcW(hwnd, msg, w, l);
    };
    let e = &mut *EDITOR;
    match msg {
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
            e.wheel_delta += ((w >> 16) as u16 as i16) as i32;
            let steps = e.wheel_delta / 120;
            e.wheel_delta %= 120;
            if steps != 0 {
                let mut info: SCROLLINFO = zeroed();
                info.cbSize = size_of::<SCROLLINFO>() as u32;
                info.fMask = SIF_ALL;
                GetScrollInfo(hwnd, SB_VERT, &mut info);
                info.fMask = SIF_POS;
                info.nPos -= steps * 72;
                // Redraw the scrollbar thumb; only the form's intermediate
                // control/background paints are suppressed during batching.
                let y = SetScrollInfo(hwnd, SB_VERT, &info, 1);
                e.scroll_to(e.scroll_x, y);
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
            if matches!(id, NAME | TARGET | ARGS | RUNNER | CWD) && notify == EN_CHANGE {
                e.item_dirty = true;
                InvalidateRect(hwnd, null(), 1);
                return 0;
            }
            if matches!(id, KIND | PARENT) && notify == CBN_SELCHANGE as u32 {
                e.item_dirty = true;
                e.enable_fields();
                return 0;
            }
            if id == TREE && notify == LBN_SELCHANGE as u32 {
                let next = e
                    .rows
                    .get(send(hwnd, TREE, LB_GETCURSEL, 0, 0) as usize)
                    .copied();
                e.checkpoint_settings();
                if let Some(next) = next {
                    match e.apply() {
                        Ok(()) => {
                            e.selected = next;
                            e.selection_to_controls();
                        }
                        Err(message) => {
                            e.status = message;
                            set(hwnd, ADVICE, &e.status);
                            let index = e.rows.iter().position(|id| *id == e.selected).unwrap_or(0);
                            send(hwnd, TREE, LB_SETCURSEL, index, 0);
                        }
                    }
                }
                InvalidateRect(hwnd, null(), 1);
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
            if notify != BN_CLICKED {
                return 0;
            }
            e.checkpoint_settings();
            let result = match id {
                BROWSE_TARGET | BROWSE_RUNNER | BROWSE_CWD => e.browse(id),
                UNDO => e.travel(false),
                REDO => e.travel(true),
                IMPORT => e.transfer(false),
                EXPORT => e.transfer(true),
                APPLY => e.apply(),
                ADD_STAR => e.add(false),
                ADD_FOLDER => e.add(true),
                DELETE => (|| {
                    e.apply()?;
                    if e.selected == e.c.root.id {
                        return Err("The root constellation cannot be deleted".into());
                    }
                    let mut c = e.c.clone();
                    remove(&mut c.root, e.selected);
                    e.commit(c);
                    e.selected = e.c.root.id;
                    e.refresh();
                    e.status =
                        "Deleted from draft. Undo restores the item and its children.".into();
                    Ok(())
                })(),
                UP => e.reorder(false),
                DOWN => e.reorder(true),
                SAVE | 1 => (|| {
                    e.apply()?;
                    e.controls_to_settings()?;
                    e.checkpoint_settings();
                    storage::save(&e.root, &e.c)?;
                    notify_resident();
                    e.saved = e.c.clone();
                    e.dirty = false;
                    e.status = "Saved. New settings apply on the next radial invocation.".into();
                    Ok(())
                })(),
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
                    let c = storage::read(&e.root)?;
                    e.commit(c.clone());
                    e.saved = c;
                    e.dirty = false;
                    e.refresh();
                    e.settings_to_controls();
                    e.status =
                        "Reloaded portable configuration. Undo restores the previous draft.".into();
                    Ok(())
                })(),
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
                SOUND => {
                    let result = e.controls_to_settings();
                    e.dirty = true;
                    result
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
            e.resize_view();
            0
        }
        WM_PAINT => {
            let measured = std::time::Instant::now();
            let mut sectors = 0;
            render::buffered(hwnd, theme::BACKGROUND, |dc| {
                SetViewportOrgEx(dc, -e.scroll_x, -e.scroll_y, null_mut());
                for r in [
                    RECT {
                        left: 10,
                        top: 8,
                        right: 290,
                        bottom: 785,
                    },
                    RECT {
                        left: 295,
                        top: 8,
                        right: 680,
                        bottom: 785,
                    },
                    RECT {
                        left: 690,
                        top: 8,
                        right: e.preview.right + 10,
                        bottom: 800,
                    },
                ] {
                    theme::panel(dc, r);
                }
                sectors = e.cached_preview(dc);
                let mut cr: RECT = zeroed();
                GetClientRect(hwnd, &mut cr);
                render::text(
                    dc,
                    &format!(
                        "{}{}",
                        if e.dirty || e.item_dirty || e.settings_invalid {
                            "Unsaved changes · "
                        } else {
                            ""
                        },
                        e.status
                    ),
                    15,
                    cr.bottom.max(895) - 32,
                    cr.right - 30,
                    theme::MUTED,
                    12,
                );
            });
            if e.measure_preview {
                e.measure_preview = false;
                let report = serde_json::json!({"sectors":sectors,"paint_ms":measured.elapsed().as_secs_f64()*1000.,
                    "conditions":"Native full-preview paint including layout, GDI drawing and warnings; excludes startup, user input and display scanout"});
                let _ = std::fs::write(
                    e.root.join("data/preview-measurement.json"),
                    serde_json::to_vec_pretty(&report).unwrap(),
                );
            }
            0
        }
        WM_CLOSE => {
            if (e.dirty || e.item_dirty || e.settings_invalid)
                && MessageBoxW(
                    hwnd,
                    wide("Discard unsaved changes and close configuration?").as_ptr(),
                    wide("Constellation").as_ptr(),
                    MB_YESNO | MB_ICONQUESTION,
                ) != IDYES
            {
                return 0;
            }
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
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
struct Profiles {
    hwnd: HWND,
    draft: Vec<Profile>,
    selected: Option<usize>,
    recording: bool,
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
            P_REMOVE,
            P_APPLY,
            P_RECORD,
        ] {
            EnableWindow(GetDlgItem(self.hwnd, id), p.is_some() as i32);
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
        for (id, on) in [(P_DIRECTIONAL, p.is_some_and(|p| p.directional))] {
            send(self.hwnd, id, BM_SETCHECK, usize::from(on), 0);
        }
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
        self.draft[i] = Profile {
            id: self.draft[i].id,
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
    if let Some(p) = &e.profiles {
        super::focus_window(p.hwnd);
        return;
    }
    // Preserve any current item and spacing edits before opening the modal draft.
    if let Err(err) = e.apply().and_then(|_| e.controls_to_settings()) {
        error(&err);
        return;
    }
    let class = wide("Constellation.Profiles");
    let module = GetModuleHandleW(null());
    let wc = WNDCLASSW {
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
        right: 810,
        bottom: 585,
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
    if hwnd.is_null() {
        error("Cannot create shortcut profile window");
        return;
    }
    theme::window(hwnd);
    e.profiles = Some(Profiles {
        hwnd,
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
        "Apply profile",
        P_APPLY,
        310,
        425,
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
        465,
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
        530,
        95,
        32,
        WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
    );
    control(
        hwnd, "BUTTON", "Cancel", P_CANCEL, 675, 530, 95, 32, WS_TABSTOP,
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
    if let Some(result) = theme::message(msg, w, l) {
        return result;
    }
    if msg == WM_PAINT {
        render::buffered(hwnd, theme::BACKGROUND, |_| {});
        return 0;
    }
    if EDITOR.is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let Some(_guard) = super::DispatchGuard::enter() else {
        if msg == super::recorder::CAPTURE {
            PostMessageW(hwnd, msg, w, l);
        }
        return DefWindowProcW(hwnd, msg, w, l);
    };
    let e = &mut *EDITOR;
    let Some(p) = e.profiles.as_mut() else {
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
            SetForegroundWindow(e.hwnd);
            InvalidateRect(e.hwnd, null(), 1);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}
