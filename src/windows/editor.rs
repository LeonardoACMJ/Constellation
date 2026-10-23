#![allow(unsafe_op_in_unsafe_fn)]
use super::{error, render, wide};
use crate::{
    config::{Config, Node, Target},
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
    System::LibraryLoader::GetModuleHandleW,
    UI::{HiDpi::*, WindowsAndMessaging::*},
};

const TREE: i32 = 100;
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
    status: String,
    preview: RECT,
    dirty: bool,
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
    CreateWindowExW(
        0,
        wide(class).as_ptr(),
        wide(label).as_ptr(),
        WS_CHILD | WS_VISIBLE | style,
        x,
        y,
        w,
        h,
        hwnd,
        id as usize as HMENU,
        GetModuleHandleW(null()),
        null(),
    )
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
        let c = storage::load_or_create(&root)?;
        let module = GetModuleHandleW(null());
        let class = wide("Constellation.Config");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: module,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
            ..zeroed()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("Constellation — Configure & Visualize").as_ptr(),
            WS_OVERLAPPEDWINDOW,
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
            c,
            rows: vec![],
            parents: vec![],
            selected,
            displays,
            monitor: 0,
            json: null_mut(),
            status: "Edit items, tune spacing, then Save. Use Advanced JSON for shortcut profiles."
                .into(),
            preview: RECT {
                left: 700,
                top: 75,
                right: 1335,
                bottom: 690,
            },
            dirty: false,
        }));
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
            (ARGS, "Arguments (JSON array)", 234),
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
                360,
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
            "Windows shortcut",
            "URL",
            "Filesystem folder",
            "Script",
        ] {
            send(hwnd, KIND, CB_ADDSTRING, 0, wide(title).as_ptr() as isize);
        }
        control(hwnd, "BUTTON", "Apply item", APPLY, 305, 490, 150, 32, 0);
        label(
            hwnd,
            "LAYOUT (numeric values; no forced fitting)",
            305,
            548,
            370,
        );
        for (id, title, x, y) in [
            (RADIUS, "Radius", 305, 575),
            (DISTANCE, "Distance × radius", 490, 575),
            (SPREAD, "Spread °", 305, 635),
            (GAP, "Gap", 490, 635),
        ] {
            label(hwnd, title, x, y, 175);
            control(
                hwnd,
                "EDIT",
                "",
                id,
                x,
                y + 20,
                170,
                28,
                WS_BORDER | ES_AUTOHSCROLL as u32 | WS_TABSTOP,
            );
        }
        control(
            hwnd,
            "BUTTON",
            "Chimes",
            SOUND,
            305,
            700,
            110,
            28,
            BS_AUTOCHECKBOX as u32,
        );
        label(hwnd, "Volume (0–1)", 430, 700, 110);
        control(
            hwnd,
            "EDIT",
            "",
            VOLUME,
            550,
            697,
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
            1010,
            13,
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
            745,
            100,
            35,
            BS_DEFPUSHBUTTON as u32,
        );
        control(hwnd, "BUTTON", "Reload", RELOAD, 130, 745, 100, 35, 0);
        control(
            hwnd,
            "BUTTON",
            "Profiles / Advanced JSON…",
            ADVANCED,
            240,
            745,
            250,
            35,
            0,
        );
        e.refresh();
        e.settings_to_controls();
        e.dirty = false;
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if !IsDialogMessageW(hwnd, &msg).is_positive() {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        drop(Box::from_raw(EDITOR));
        EDITOR = null_mut();
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
    unsafe fn selection_to_controls(&self) {
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
        set(self.hwnd, ARGS, &serde_json::to_string(&args).unwrap());
        set(self.hwnd, RUNNER, &runner);
        set(self.hwnd, CWD, &cwd);
        let parent = find_parent(&self.c.root, self.selected).unwrap_or(self.c.root.id);
        let idx = self
            .parents
            .iter()
            .position(|id| *id == parent)
            .unwrap_or(0);
        send(self.hwnd, PARENT, CB_SETCURSEL, idx, 0);
    }
    unsafe fn settings_to_controls(&self) {
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
        self.c = candidate;
        Ok(())
    }
    unsafe fn apply(&mut self) -> Result<(), String> {
        let mut candidate = self.c.clone();
        let old = candidate
            .find(self.selected)
            .ok_or("Select an item")?
            .clone();
        let kind = send(self.hwnd, KIND, CB_GETCURSEL, 0, 0);
        let path = get(self.hwnd, TARGET);
        let args: Vec<String> = serde_json::from_str(&get(self.hwnd, ARGS))
            .map_err(|_| "Arguments must be a JSON string array, e.g. [\"--flag\"]")?;
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
        self.c = candidate;
        self.dirty = true;
        self.refresh();
        Ok(())
    }
    unsafe fn add(&mut self, folder: bool) {
        let parent = if self
            .c
            .find(self.selected)
            .is_some_and(|n| n.children.is_some())
        {
            self.selected
        } else {
            find_parent(&self.c.root, self.selected).unwrap_or(self.c.root.id)
        };
        let id = self.c.root.max_id() + 1;
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
        self.selected = id;
        self.dirty = true;
        self.refresh();
    }
    unsafe fn reorder(&mut self, down: bool) {
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
            self.dirty = true;
            self.refresh();
        }
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
        hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
        ..zeroed()
    };
    RegisterClassW(&wc);
    let hwnd = CreateWindowExW(
        0,
        class.as_ptr(),
        wide("Constellation — Profiles & Advanced JSON").as_ptr(),
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
    e.json = hwnd;
    label(
        hwnd,
        "Edit profiles, bindings and advanced values. Save validates and writes only inside the portable folder.",
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
                let c: Config =
                    serde_json::from_str(&get(hwnd, JSON_TEXT)).map_err(|x| x.to_string())?;
                storage::save(&e.root, &c)?;
                notify_resident();
                e.c = c;
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
            DestroyWindow(hwnd);
            e.json = null_mut();
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_DESTROY {
        PostQuitMessage(0);
        return 0;
    }
    if EDITOR.is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let Some(_guard) = super::DispatchGuard::enter() else {
        if msg == WM_COMMAND && (w >> 16) as u32 == BN_CLICKED && (w & 0xffff) >= APPLY as usize {
            PostMessageW(hwnd, msg, w, l);
        }
        return DefWindowProcW(hwnd, msg, w, l);
    };
    let e = &mut *EDITOR;
    match msg {
        WM_COMMAND => {
            let id = (w & 0xffff) as i32;
            let notify = (w >> 16) as u32;
            if id == TREE && notify == LBN_SELCHANGE as u32 {
                let i = send(hwnd, TREE, LB_GETCURSEL, 0, 0);
                if let Some(id) = e.rows.get(i as usize) {
                    e.selected = *id;
                    e.selection_to_controls();
                }
                return 0;
            }
            if id == MONITOR && notify == CBN_SELCHANGE as u32 {
                e.monitor = send(hwnd, MONITOR, CB_GETCURSEL, 0, 0) as usize;
                InvalidateRect(hwnd, null(), 1);
                return 0;
            }
            if matches!(id, RADIUS | DISTANCE | SPREAD | GAP | VOLUME) && notify == EN_CHANGE {
                if e.controls_to_settings().is_ok() {
                    e.dirty = true;
                    InvalidateRect(hwnd, &e.preview, 1);
                }
                return 0;
            }
            if notify != BN_CLICKED {
                return 0;
            }
            let result = match id {
                APPLY => e.apply(),
                ADD_STAR => {
                    e.add(false);
                    Ok(())
                }
                ADD_FOLDER => {
                    e.add(true);
                    Ok(())
                }
                DELETE => {
                    if e.selected == e.c.root.id {
                        Err("The root constellation cannot be deleted".into())
                    } else {
                        if MessageBoxW(hwnd,wide("Delete this item and all its children? Changes are not persisted until Save.").as_ptr(),wide("Delete item").as_ptr(),MB_YESNO|MB_ICONQUESTION)==IDYES{remove(&mut e.c.root,e.selected);e.selected=e.c.root.id;e.dirty=true;e.refresh();}
                        Ok(())
                    }
                }
                UP => {
                    e.reorder(false);
                    Ok(())
                }
                DOWN => {
                    e.reorder(true);
                    Ok(())
                }
                SAVE => (|| {
                    e.apply()?;
                    e.controls_to_settings()?;
                    storage::save(&e.root, &e.c)?;
                    notify_resident();
                    e.dirty = false;
                    e.status = "Saved. New settings apply on the next radial invocation.".into();
                    Ok(())
                })(),
                RELOAD => match storage::read(&e.root) {
                    Ok(c) => {
                        e.c = c;
                        e.dirty = false;
                        e.refresh();
                        e.settings_to_controls();
                        e.status = "Reloaded portable configuration.".into();
                        Ok(())
                    }
                    Err(s) => Err(s),
                },
                ADVANCED => {
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
                error(&s);
            }
            InvalidateRect(hwnd, null(), 1);
            0
        }
        WM_SIZE => {
            let width = (l as u32 & 0xffff) as i32;
            e.preview.right = (width - 30).max(e.preview.left + 100);
            InvalidateRect(hwnd, null(), 1);
            0
        }
        WM_PAINT => {
            let mut ps = zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
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
                render::rgb(100, 105, 115),
                12,
            );
            let mut cr: RECT = zeroed();
            GetClientRect(hwnd, &mut cr);
            render::text(
                dc,
                &format!(
                    "{}{}",
                    if e.dirty { "Unsaved changes · " } else { "" },
                    e.status
                ),
                15,
                cr.bottom - 32,
                cr.right - 30,
                render::rgb(85, 90, 105),
                12,
            );
            EndPaint(hwnd, &ps);
            0
        }
        WM_CLOSE => {
            if e.dirty
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
