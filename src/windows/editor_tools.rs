//! Owned, modeless configuration tools. Native modal pickers never retain a
//! borrow of editor/tool state or the native dispatch guard.
#![allow(unsafe_op_in_unsafe_fn)]
use super::{editor_theme as theme, render, wide};
use crate::{
    config::Config,
    organize::{self, PathRef, Row},
    theme::{Color, Palette},
};
use std::{
    mem::{size_of, zeroed},
    path::PathBuf,
    ptr::{null, null_mut},
    sync::Arc,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    Storage::FileSystem::SearchPathW,
    System::{Environment::ExpandEnvironmentStringsW, LibraryLoader::GetModuleHandleW},
    UI::{
        Controls::Dialogs::*, HiDpi::*, Input::KeyboardAndMouse::EnableWindow,
        WindowsAndMessaging::*,
    },
};
pub const APPLY: u32 = WM_APP + 70;
pub const CLOSED: u32 = WM_APP + 71;
const PATH_READY: u32 = WM_APP + 72;
const COLOR_PICKED: u32 = WM_APP + 73;
pub const THEME: u32 = 701;
pub const MOVE: u32 = 702;
pub const PATHS: u32 = 703;
const OK: i32 = 1;
const CANCEL: i32 = 2;
const STATUS: i32 = 4;
const PRESET: i32 = 5;
const PALETTE_NAME: i32 = 7;
const PALETTE_SAVE: i32 = 8;
const RESET: i32 = 6;
const SOURCES: i32 = 10;
const FILTER: i32 = 11;
const DESTINATIONS: i32 = 12;
const OLD: i32 = 20;
const NEW: i32 = 21;
const SCAN: i32 = 22;
const PREVIEW: i32 = 23;
const REPORT: i32 = 24;
pub enum Action {
    Theme(Palette),
    Move(Vec<u64>, u64),
    Relink(String, String),
}
enum Kind {
    Theme {
        palette: Palette,
        saved: Vec<crate::palettes::NamedPalette>,
    },
    Move {
        sources: Vec<Row>,
        destinations: Vec<Row>,
        filtered: Vec<u64>,
    },
    Paths {
        config: Arc<Config>,
        universe: usize,
        worker: Option<crate::background::Latest<(), Vec<(PathRef, bool)>>>,
        plan: Option<(String, String)>,
    },
}
struct Tools {
    owner: HWND,
    root: PathBuf,
    kind: Kind,
    result: Option<Action>,
}
unsafe fn state(hwnd: HWND) -> Option<&'static mut Tools> {
    (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Tools).as_mut()
}
unsafe fn text(hwnd: HWND, id: i32) -> String {
    let h = GetDlgItem(hwnd, id);
    let n = GetWindowTextLengthW(h).max(0) as usize;
    let mut b = vec![0; n + 1];
    GetWindowTextW(h, b.as_mut_ptr(), b.len() as i32);
    String::from_utf16_lossy(&b[..n])
}
unsafe fn set(hwnd: HWND, id: i32, value: &str) {
    SetWindowTextW(GetDlgItem(hwnd, id), wide(value).as_ptr());
}
unsafe fn send(hwnd: HWND, id: i32, msg: u32, w: usize, l: isize) -> isize {
    SendMessageW(GetDlgItem(hwnd, id), msg, w, l)
}
unsafe fn control(
    hwnd: HWND,
    class: &str,
    title: &str,
    id: i32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    style: u32,
) {
    let scale = GetDpiForWindow(hwnd) as f64 / 96.;
    let px = |v: i32| (v as f64 * scale).round() as i32;
    let style = if class == "BUTTON" && matches!(style & BS_TYPEMASK as u32, 0 | 1) {
        (style & !(BS_TYPEMASK as u32)) | BS_OWNERDRAW as u32
    } else {
        style
    };
    let child = CreateWindowExW(
        0,
        wide(class).as_ptr(),
        wide(title).as_ptr(),
        WS_CHILD | WS_VISIBLE | style,
        px(x),
        px(y),
        px(w),
        px(h),
        hwnd,
        id as usize as HMENU,
        GetModuleHandleW(null()),
        null(),
    );
    theme::control(child, class);
}
unsafe fn label(hwnd: HWND, value: &str, x: i32, y: i32, w: i32) {
    control(hwnd, "STATIC", value, 0, x, y, w, 22, 0);
}
pub unsafe fn error(hwnd: HWND, value: &str) {
    set(hwnd, STATUS, value);
}
pub unsafe fn take_result(hwnd: HWND) -> Option<Action> {
    state(hwnd)?.result.take()
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Recovery {
    kind: u32,
    fields: Vec<(i32, String)>,
    selected: Vec<u64>,
}
pub unsafe fn capture(hwnd: HWND) -> Option<String> {
    let (kind, ids) = match &state(hwnd)?.kind {
        Kind::Theme { .. } => (
            THEME,
            (100..113)
                .chain(std::iter::once(PALETTE_NAME))
                .collect::<Vec<_>>(),
        ),
        Kind::Move { .. } => (MOVE, vec![FILTER]),
        Kind::Paths { .. } => (PATHS, vec![OLD, NEW]),
    };
    let fields = ids.into_iter().map(|id| (id, text(hwnd, id))).collect();
    let selected = match &state(hwnd)?.kind {
        Kind::Move { sources, .. } => sources
            .iter()
            .enumerate()
            .filter(|(i, _)| send(hwnd, SOURCES, LB_GETSEL, *i, 0) > 0)
            .map(|(_, r)| r.id)
            .collect(),
        _ => vec![],
    };
    serde_json::to_string(&Recovery {
        kind,
        fields,
        selected,
    })
    .ok()
}
pub fn recovery_kind(value: &str) -> Option<u32> {
    let recovery: Recovery = serde_json::from_str(value).ok()?;
    matches!(recovery.kind, THEME | MOVE | PATHS).then_some(recovery.kind)
}
pub unsafe fn restore(hwnd: HWND, value: &str) {
    if let Ok(recovery) = serde_json::from_str::<Recovery>(value) {
        for (id, value) in recovery.fields {
            if (100..113).contains(&id) || matches!(id, PALETTE_NAME | FILTER | OLD | NEW) {
                set(hwnd, id, &value);
            }
        }
        if let Some(Tools {
            kind: Kind::Move { sources, .. },
            ..
        }) = state(hwnd)
        {
            for (i, row) in sources.iter().enumerate() {
                send(
                    hwnd,
                    SOURCES,
                    LB_SETSEL,
                    recovery.selected.contains(&row.id) as usize,
                    i as isize,
                );
            }
        }
    }
}
pub unsafe fn open(
    owner: HWND,
    root: PathBuf,
    config: Config,
    universe: usize,
    selected: u64,
    kind: u32,
) -> Result<HWND, String> {
    let module = GetModuleHandleW(null());
    let class = wide("Constellation.Tools");
    RegisterClassW(&WNDCLASSW {
        hIcon: super::icons::large(),
        lpfnWndProc: Some(proc),
        hInstance: module,
        lpszClassName: class.as_ptr(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        ..zeroed()
    });
    let scale = GetDpiForWindow(owner) as f64 / 96.;
    let mut bounds = RECT {
        left: 0,
        top: 0,
        right: (940. * scale).round() as i32,
        bottom: (570. * scale).round() as i32,
    };
    let style = WS_CAPTION | WS_SYSMENU | WS_OVERLAPPED | WS_CLIPCHILDREN;
    AdjustWindowRectExForDpi(&mut bounds, style, 0, 0, GetDpiForWindow(owner));
    let mut owner_rect = zeroed();
    GetWindowRect(owner, &mut owner_rect);
    let mut monitor: MONITORINFO = zeroed();
    monitor.cbSize = size_of::<MONITORINFO>() as u32;
    GetMonitorInfoW(
        MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST),
        &mut monitor,
    );
    let w = bounds.right - bounds.left;
    let h = bounds.bottom - bounds.top;
    let work = monitor.rcWork;
    let x = (owner_rect.left + (owner_rect.right - owner_rect.left - w) / 2)
        .clamp(work.left, work.left + (work.right - work.left - w).max(0));
    let y = (owner_rect.top + (owner_rect.bottom - owner_rect.top - h) / 2)
        .clamp(work.top, work.top + (work.bottom - work.top - h).max(0));
    let title = match kind {
        THEME => "Constellation — Themes & colors",
        MOVE => "Constellation — Move items",
        _ => "Constellation — Check & repair paths",
    };
    let hwnd = CreateWindowExW(
        WS_EX_COMPOSITED,
        class.as_ptr(),
        wide(title).as_ptr(),
        style,
        x,
        y,
        w,
        h,
        owner,
        null_mut(),
        module,
        null(),
    );
    if hwnd.is_null() {
        return Err("Cannot create configuration tool".into());
    }
    theme::window(hwnd);
    super::icons::set_small(hwnd);
    let library = if kind == THEME {
        crate::palettes::load(&root)
    } else {
        Ok(vec![])
    };
    let library_error = library.as_ref().err().cloned();
    let tool_kind = match kind {
        THEME => Kind::Theme {
            palette: config.theme,
            saved: library.unwrap_or_default(),
        },
        MOVE => {
            let all = organize::rows(&config);
            Kind::Move {
                sources: all
                    .iter()
                    .filter(|r| r.universe == universe && r.id != config.universes[universe].id)
                    .cloned()
                    .collect(),
                destinations: all.into_iter().filter(|r| r.branch).collect(),
                filtered: vec![],
            }
        }
        _ => Kind::Paths {
            config: Arc::new(config.clone()),
            universe,
            worker: None,
            plan: None,
        },
    };
    SetWindowLongPtrW(
        hwnd,
        GWLP_USERDATA,
        Box::into_raw(Box::new(Tools {
            owner,
            root: root.clone(),
            kind: tool_kind,
            result: None,
        })) as isize,
    );
    match kind {
        THEME => {
            label(hwnd, "Palette", 20, 18, 80);
            control(
                hwnd,
                "COMBOBOX",
                "",
                PRESET,
                110,
                14,
                230,
                150,
                CBS_DROPDOWNLIST as u32 | WS_TABSTOP,
            );
            refresh_presets(hwnd);
            control(hwnd, "BUTTON", "Reset", RESET, 355, 14, 110, 28, WS_TABSTOP);
            label(hwnd, "Name", 495, 18, 50);
            control(
                hwnd,
                "EDIT",
                "",
                PALETTE_NAME,
                555,
                14,
                205,
                28,
                WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
            );
            send(
                hwnd,
                PALETTE_NAME,
                windows_sys::Win32::UI::Controls::EM_SETLIMITTEXT,
                128,
                0,
            );
            control(
                hwnd,
                "BUTTON",
                "Save palette",
                PALETTE_SAVE,
                775,
                14,
                140,
                28,
                WS_TABSTOP,
            );
            for (i, name) in Palette::LABELS.iter().enumerate() {
                let x = 20 + (i / 7) as i32 * 265;
                let y = 65 + (i % 7) as i32 * 57;
                label(hwnd, name, x, y, 245);
                control(
                    hwnd,
                    "EDIT",
                    "",
                    100 + i as i32,
                    x,
                    y + 22,
                    145,
                    27,
                    WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
                );
                control(
                    hwnd,
                    "BUTTON",
                    "Choose…",
                    200 + i as i32,
                    x + 153,
                    y + 22,
                    90,
                    27,
                    WS_TABSTOP,
                );
            }
            fill_palette(hwnd, config.theme);
            let built_in = (0..4).find(|i| Palette::preset(*i) == config.theme);
            if let Some(index) = built_in {
                send(hwnd, PRESET, CB_SETCURSEL, index, 0);
            } else {
                let matching = state(hwnd).and_then(|tool| {
                    if let Kind::Theme { saved, .. } = &tool.kind {
                        saved
                            .iter()
                            .enumerate()
                            .find(|(_, entry)| entry.palette == config.theme)
                            .map(|(index, entry)| (index, entry.name.clone()))
                    } else {
                        None
                    }
                });
                if let Some((index, name)) = matching {
                    send(hwnd, PRESET, CB_SETCURSEL, index + 4, 0);
                    set(hwnd, PALETTE_NAME, &name);
                    palette_save_label(hwnd);
                }
            }
            label(
                hwnd,
                "Live preview · changes stay in this draft until Save",
                555,
                55,
                360,
            );
            label(
                hwnd,
                "High-contrast Windows colors take precedence while enabled.",
                20,
                475,
                890,
            );
        }
        MOVE => {
            label(
                hwnd,
                "Items · Ctrl/Shift selects several; a selected branch includes its children",
                20,
                15,
                890,
            );
            control(
                hwnd,
                "LISTBOX",
                "",
                SOURCES,
                20,
                44,
                430,
                420,
                WS_BORDER
                    | WS_VSCROLL
                    | WS_HSCROLL
                    | WS_TABSTOP
                    | LBS_EXTENDEDSEL as u32
                    | LBS_NOTIFY as u32,
            );
            if let Kind::Move { sources, .. } = &state(hwnd).unwrap().kind {
                for (i, row) in sources.iter().enumerate() {
                    send(
                        hwnd,
                        SOURCES,
                        LB_ADDSTRING,
                        0,
                        wide(&row.path).as_ptr() as isize,
                    );
                    if row.id == selected {
                        send(hwnd, SOURCES, LB_SETSEL, 1, i as isize);
                    }
                }
            }
            label(
                hwnd,
                "Destination · search the full constellation path",
                475,
                15,
                440,
            );
            control(
                hwnd,
                "EDIT",
                "",
                FILTER,
                475,
                44,
                440,
                28,
                WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
            );
            control(
                hwnd,
                "LISTBOX",
                "",
                DESTINATIONS,
                475,
                83,
                440,
                381,
                WS_BORDER | WS_VSCROLL | WS_HSCROLL | WS_TABSTOP | LBS_NOTIFY as u32,
            );
            filter_destinations(hwnd);
            label(
                hwnd,
                "Moves are one Undo step. Entry-point and visibility dependencies are preserved.",
                20,
                475,
                890,
            );
        }
        _ => {
            label(
                hwnd,
                "Check paths without launching anything. Checks run separately from the editor.",
                20,
                15,
                890,
            );
            control(
                hwnd,
                "BUTTON",
                "Check paths",
                SCAN,
                20,
                44,
                150,
                29,
                WS_TABSTOP,
            );
            label(hwnd, "Old folder prefix", 190, 20, 340);
            control(
                hwnd,
                "EDIT",
                "",
                OLD,
                190,
                44,
                340,
                28,
                WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
            );
            label(hwnd, "New folder prefix", 550, 20, 365);
            control(
                hwnd,
                "EDIT",
                "",
                NEW,
                550,
                44,
                365,
                28,
                WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
            );
            control(
                hwnd,
                "BUTTON",
                "Preview replacements",
                PREVIEW,
                20,
                84,
                215,
                29,
                WS_TABSTOP,
            );
            control(
                hwnd,
                "LISTBOX",
                "",
                REPORT,
                20,
                125,
                895,
                340,
                WS_BORDER | WS_VSCROLL | WS_HSCROLL | WS_TABSTOP,
            );
            label(
                hwnd,
                "Only reviewed path fields change; URLs, arguments and notes are never rewritten.",
                20,
                475,
                890,
            );
            let refs = organize::paths(&config, universe);
            let window = hwnd as usize;
            let worker = crate::background::Latest::new(
                move |_: &()| {
                    refs.iter()
                        .cloned()
                        .map(|reference| {
                            let ok = path_exists(&root, &reference);
                            (reference, ok)
                        })
                        .collect()
                },
                move || {
                    PostMessageW(window as HWND, PATH_READY, 0, 0);
                },
            )
            .ok();
            if let Kind::Paths { worker: slot, .. } = &mut state(hwnd).unwrap().kind {
                *slot = worker;
            }
            EnableWindow(GetDlgItem(hwnd, OK), 0);
        }
    }
    let caption = match kind {
        THEME => "Use palette in draft",
        MOVE => "Move selected",
        _ => "Apply reviewed replacements",
    };
    control(
        hwnd,
        "BUTTON",
        caption,
        OK,
        20,
        525,
        265,
        32,
        WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
    );
    control(
        hwnd, "BUTTON", "Cancel", CANCEL, 300, 525, 130, 32, WS_TABSTOP,
    );
    control(hwnd, "STATIC", "", STATUS, 20, 498, 895, 22, 0);
    if let Some(message) = library_error {
        error(hwnd, &message);
    }
    if kind == PATHS {
        EnableWindow(GetDlgItem(hwnd, OK), 0);
        start_scan(hwnd);
    }
    for id in [SOURCES, DESTINATIONS, REPORT] {
        if !GetDlgItem(hwnd, id).is_null() {
            send(hwnd, id, LB_SETHORIZONTALEXTENT, 2400, 0);
        }
    }
    EnableWindow(owner, 0);
    ShowWindow(hwnd, SW_SHOW);
    super::focus_window(hwnd);
    Ok(hwnd)
}
unsafe fn refresh_presets(hwnd: HWND) {
    let names: Vec<String> = state(hwnd)
        .and_then(|t| {
            if let Kind::Theme { saved, .. } = &t.kind {
                Some(saved.iter().map(|p| p.name.clone()).collect())
            } else {
                None
            }
        })
        .unwrap_or_default();
    send(hwnd, PRESET, CB_RESETCONTENT, 0, 0);
    for name in crate::palettes::BUILTINS
        .iter()
        .map(|s| s.to_string())
        .chain(names)
    {
        send(hwnd, PRESET, CB_ADDSTRING, 0, wide(name).as_ptr() as isize);
    }
}
unsafe fn palette_save_label(hwnd: HWND) {
    let name = text(hwnd, PALETTE_NAME);
    let replace = state(hwnd).is_some_and(|t| {
        if let Kind::Theme { saved, .. } = &t.kind {
            saved
                .iter()
                .any(|p| p.name.to_lowercase() == name.trim().to_lowercase())
        } else {
            false
        }
    });
    set(
        hwnd,
        PALETTE_SAVE,
        if replace {
            "Replace palette"
        } else {
            "Save palette"
        },
    );
}
unsafe fn fill_palette(hwnd: HWND, palette: Palette) {
    for (i, c) in palette.colors().iter().enumerate() {
        set(hwnd, 100 + i as i32, &c.hex());
    }
    if let Kind::Theme { palette: p, .. } = &mut state(hwnd).unwrap().kind {
        *p = palette;
    }
    InvalidateRect(hwnd, null(), 0);
}
unsafe fn read_palette(hwnd: HWND) -> Result<Palette, String> {
    let mut palette = Palette::default();
    for i in 0..13 {
        palette.set(i, Color::parse(&text(hwnd, 100 + i as i32))?);
    }
    Ok(palette)
}
unsafe fn filter_destinations(hwnd: HWND) {
    let filter = text(hwnd, FILTER).to_lowercase();
    if let Kind::Move {
        destinations,
        filtered,
        ..
    } = &mut state(hwnd).unwrap().kind
    {
        filtered.clear();
        send(hwnd, DESTINATIONS, LB_RESETCONTENT, 0, 0);
        for row in destinations {
            if row.path.to_lowercase().contains(&filter) {
                filtered.push(row.id);
                send(
                    hwnd,
                    DESTINATIONS,
                    LB_ADDSTRING,
                    0,
                    wide(&row.path).as_ptr() as isize,
                );
            }
        }
        if !filtered.is_empty() {
            send(hwnd, DESTINATIONS, LB_SETCURSEL, 0, 0);
        }
    }
}
unsafe fn path_exists(root: &std::path::Path, reference: &PathRef) -> bool {
    let mut expanded = vec![0u16; 32768];
    let n = ExpandEnvironmentStringsW(
        wide(&reference.value).as_ptr(),
        expanded.as_mut_ptr(),
        expanded.len() as u32,
    );
    let value = if n > 0 && n as usize <= expanded.len() {
        String::from_utf16_lossy(&expanded[..n as usize - 1])
    } else {
        reference.value.clone()
    };
    if crate::storage::resolve(root, &value).exists() {
        return true;
    }
    if reference.command {
        let mut buffer = vec![0u16; 32768];
        for ext in [None, Some(".exe"), Some(".cmd")] {
            let ext = ext.map(wide);
            if SearchPathW(
                null(),
                wide(&value).as_ptr(),
                ext.as_ref().map_or(null(), |v| v.as_ptr()),
                buffer.len() as u32,
                buffer.as_mut_ptr(),
                null_mut(),
            ) > 0
            {
                return true;
            }
        }
    }
    false
}
unsafe fn start_scan(hwnd: HWND) {
    if let Kind::Paths { worker, plan, .. } = &mut state(hwnd).unwrap().kind {
        *plan = None;
        if let Some(worker) = worker {
            worker.invalidate();
            worker.get(&());
        }
        error(hwnd, "Checking paths…");
        EnableWindow(GetDlgItem(hwnd, OK), 0);
    }
}
unsafe fn proc_impl(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> Option<LRESULT> {
    if msg == WM_PAINT {
        render::buffered(hwnd, theme::background(), |dc| {
            theme::logical_dc(dc, GetDpiForWindow(hwnd));
            if let Some(Tools {
                kind: Kind::Theme { palette, .. },
                ..
            }) = state(hwnd)
            {
                let mut config = Config::default();
                config.theme = *palette;
                config.layout.radius = 110.;
                let sectors = crate::geometry::visible(&config, &[2]);
                let scale = 1.;
                render::draw_with_icons(
                    dc,
                    &config,
                    &config.universes[0],
                    &sectors,
                    &[2],
                    Some(3),
                    crate::geometry::Point {
                        x: 735. * scale,
                        y: 270. * scale,
                    },
                    scale,
                    true,
                    &Default::default(),
                );
            }
        });
        return Some(0);
    }
    if msg == COLOR_PICKED {
        set(hwnd, 100 + w as i32, &Color(l as u32).hex());
        return Some(0);
    }
    if msg == PATH_READY {
        if let Some(Tools {
            kind: Kind::Paths { worker, plan, .. },
            ..
        }) = state(hwnd)
        {
            if plan.is_none() {
                if let Some(rows) = worker.as_ref().and_then(|worker| worker.get(&())) {
                    send(hwnd, REPORT, LB_RESETCONTENT, 0, 0);
                    let missing = rows.iter().filter(|(_, ok)| !*ok).count();
                    for (r, ok) in rows {
                        let value = format!(
                            "{} | {} | {} | {}",
                            if ok { "OK" } else { "Missing" },
                            r.label,
                            r.field,
                            r.value
                        );
                        send(hwnd, REPORT, LB_ADDSTRING, 0, wide(value).as_ptr() as isize);
                    }
                    error(
                        hwnd,
                        &format!(
                            "Check complete: {missing} missing/unavailable paths. Nothing was launched or changed."
                        ),
                    );
                }
            }
        }
        return Some(0);
    }
    if msg == WM_CLOSE {
        DestroyWindow(hwnd);
        return Some(0);
    }
    if msg != WM_COMMAND {
        return None;
    }
    let id = (w & 0xffff) as i32;
    let notify = w >> 16;
    if id == CANCEL || id == 2 {
        DestroyWindow(hwnd);
        return Some(0);
    }
    let kind = state(hwnd)
        .map(|tool| match tool.kind {
            Kind::Theme { .. } => THEME,
            Kind::Move { .. } => MOVE,
            Kind::Paths { .. } => PATHS,
        })
        .unwrap_or(0);
    let mut result = None;
    match kind {
        THEME => {
            if id == PRESET && notify == CBN_SELCHANGE as usize {
                let index = send(hwnd, PRESET, CB_GETCURSEL, 0, 0);
                let selected = if index >= 4 {
                    state(hwnd).and_then(|t| {
                        if let Kind::Theme { saved, .. } = &t.kind {
                            saved.get(index as usize - 4).cloned()
                        } else {
                            None
                        }
                    })
                } else {
                    None
                };
                if let Some(entry) = selected {
                    fill_palette(hwnd, entry.palette);
                    set(hwnd, PALETTE_NAME, &entry.name);
                } else {
                    fill_palette(hwnd, Palette::preset(index.max(0) as usize));
                    set(hwnd, PALETTE_NAME, "");
                }
                palette_save_label(hwnd);
            } else if id == PALETTE_NAME && notify == EN_CHANGE as usize {
                palette_save_label(hwnd);
            } else if id == PALETTE_SAVE {
                let result = (|| -> Result<(), String> {
                    let palette = read_palette(hwnd)?;
                    let name = text(hwnd, PALETTE_NAME);
                    let root = state(hwnd).unwrap().root.clone();
                    let saved = crate::palettes::save_named(&root, &name, palette)?;
                    let index = saved
                        .iter()
                        .position(|p| p.name.eq_ignore_ascii_case(name.trim()))
                        .unwrap();
                    if let Kind::Theme { saved: slot, .. } = &mut state(hwnd).unwrap().kind {
                        *slot = saved;
                    }
                    refresh_presets(hwnd);
                    send(hwnd, PRESET, CB_SETCURSEL, index + 4, 0);
                    set(hwnd, PALETTE_NAME, name.trim());
                    palette_save_label(hwnd);
                    error(
                        hwnd,
                        "Palette saved in the portable library. Use palette in draft, then Save to apply it.",
                    );
                    Ok(())
                })();
                if let Err(message) = result {
                    error(hwnd, &message);
                }
            } else if id == RESET {
                fill_palette(hwnd, Palette::default());
                send(hwnd, PRESET, CB_SETCURSEL, 0, 0);
                set(hwnd, PALETTE_NAME, "");
            } else if (100..113).contains(&id) && notify == EN_CHANGE as usize {
                match read_palette(hwnd) {
                    Ok(p) => {
                        if let Some(Tools {
                            kind: Kind::Theme { palette, .. },
                            ..
                        }) = state(hwnd)
                        {
                            *palette = p;
                        }
                        error(hwnd, "");
                        InvalidateRect(hwnd, null(), 0);
                    }
                    Err(e) => error(hwnd, &e),
                }
            } else if id == OK {
                match read_palette(hwnd) {
                    Ok(p) => result = Some(Action::Theme(p)),
                    Err(e) => error(hwnd, &e),
                }
            }
        }
        MOVE => {
            if id == FILTER && notify == EN_CHANGE as usize {
                filter_destinations(hwnd);
            } else if id == OK {
                let (sources, filtered) = match &state(hwnd).unwrap().kind {
                    Kind::Move {
                        sources, filtered, ..
                    } => (
                        sources.iter().map(|r| r.id).collect::<Vec<_>>(),
                        filtered.clone(),
                    ),
                    _ => unreachable!(),
                };
                let ids: Vec<_> = sources
                    .into_iter()
                    .enumerate()
                    .filter(|(i, _)| send(hwnd, SOURCES, LB_GETSEL, *i, 0) > 0)
                    .map(|(_, id)| id)
                    .collect();
                let index = send(hwnd, DESTINATIONS, LB_GETCURSEL, 0, 0);
                if ids.is_empty() || index < 0 {
                    error(hwnd, "Choose items and a destination");
                } else if let Some(dest) = filtered.get(index as usize) {
                    result = Some(Action::Move(ids, *dest));
                }
            }
        }
        PATHS => {
            if matches!(id, OLD | NEW) && notify == EN_CHANGE as usize {
                if let Kind::Paths { plan, .. } = &mut state(hwnd).unwrap().kind {
                    *plan = None;
                }
                EnableWindow(GetDlgItem(hwnd, OK), 0);
            } else if id == SCAN {
                start_scan(hwnd);
            } else if id == PREVIEW {
                let (old, new) = (text(hwnd, OLD), text(hwnd, NEW));
                let (config, universe) = match &state(hwnd).unwrap().kind {
                    Kind::Paths {
                        config, universe, ..
                    } => (config.clone(), *universe),
                    _ => unreachable!(),
                };
                match organize::relink_preview(&config, universe, &old, &new) {
                    Ok(changes) => {
                        send(hwnd, REPORT, LB_RESETCONTENT, 0, 0);
                        for (r, new) in &changes {
                            send(
                                hwnd,
                                REPORT,
                                LB_ADDSTRING,
                                0,
                                wide(format!(
                                    "{} | {} | {} -> {}",
                                    r.label, r.field, r.value, new
                                ))
                                .as_ptr() as isize,
                            );
                        }
                        error(
                            hwnd,
                            &format!(
                                "{} field replacements. Review the list, then Apply.",
                                changes.len()
                            ),
                        );
                        EnableWindow(GetDlgItem(hwnd, OK), (!changes.is_empty()) as i32);
                        if let Kind::Paths { plan, .. } = &mut state(hwnd).unwrap().kind {
                            *plan = Some((old, new));
                        }
                    }
                    Err(e) => error(hwnd, &e),
                }
            } else if id == OK {
                let plan = match &state(hwnd).unwrap().kind {
                    Kind::Paths { plan, .. } => plan.clone(),
                    _ => None,
                };
                if let Some((old, new)) = plan {
                    if old == text(hwnd, OLD) && new == text(hwnd, NEW) {
                        result = Some(Action::Relink(old, new));
                    }
                }
            }
        }
        _ => {}
    }
    if let Some(result) = result {
        let owner = {
            let tool = state(hwnd).unwrap();
            tool.result = Some(result);
            tool.owner
        };
        PostMessageW(owner, APPLY, hwnd as usize, 0);
    }
    Some(0)
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(result) = theme::display_message(hwnd, msg, w, l) {
        return result;
    }
    if msg == WM_COMMAND || msg == COLOR_PICKED {
        if let Some(owner) = state(hwnd).map(|t| t.owner) {
            SetTimer(owner, 54, 500, None);
        }
    }
    if msg == WM_NCDESTROY {
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Tools;
        if !pointer.is_null() {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            let tool = Box::from_raw(pointer);
            PostMessageW(tool.owner, CLOSED, hwnd as usize, 0);
        }
        return DefWindowProcW(hwnd, msg, w, l);
    }
    if let Some(result) = theme::message(msg, w, l) {
        return result;
    }
    let Some(guard) = super::DispatchGuard::enter() else {
        if matches!(msg, WM_COMMAND | WM_CLOSE | PATH_READY | COLOR_PICKED) {
            super::defer_message(hwnd, msg, w, l);
            return 0;
        }
        return DefWindowProcW(hwnd, msg, w, l);
    };
    if msg == WM_COMMAND && (200..213).contains(&((w & 0xffff) as i32)) {
        let index = (w & 0xffff) as usize - 200;
        let initial = state(hwnd).and_then(|t| {
            if let Kind::Theme { palette, .. } = &t.kind {
                Some(palette.colors()[index].gdi())
            } else {
                None
            }
        });
        drop(guard);
        if let Some(initial) = initial {
            let mut custom = [0u32; 16];
            let mut choose: CHOOSECOLORW = zeroed();
            choose.lStructSize = size_of::<CHOOSECOLORW>() as u32;
            choose.hwndOwner = hwnd;
            choose.rgbResult = initial;
            choose.lpCustColors = custom.as_mut_ptr();
            choose.Flags = CC_RGBINIT | CC_FULLOPEN;
            if ChooseColorW(&mut choose) != 0 {
                let c = choose.rgbResult;
                let color = ((c & 255) << 16) | (c & 0xff00) | (c >> 16);
                PostMessageW(hwnd, COLOR_PICKED, index, color as isize);
            }
        }
        return 0;
    }
    if let Some(result) = proc_impl(hwnd, msg, w, l) {
        return result;
    }
    drop(guard);
    DefWindowProcW(hwnd, msg, w, l)
}
