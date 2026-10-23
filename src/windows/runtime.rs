#![allow(unsafe_op_in_unsafe_fn)]
use super::{audio::Chimes, editor, render, target, wide};
use crate::{
    config::{Binding, Config, Input, Mode},
    engine::{Action, Engine},
    geometry::Point,
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
    System::{LibraryLoader::GetModuleHandleW, Threading::*},
    UI::{
        Accessibility::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*,
    },
};

const TRAY: u32 = WM_APP + 1;
const INVOKE: u32 = WM_APP + 2;
const RELEASE: u32 = WM_APP + 3;
const KEY_ACTION: u32 = WM_APP + 4;
const FOREGROUND: u32 = WM_APP + 5;
const POINTER_MOVE: u32 = WM_APP + 6;
const CLICK: u32 = WM_APP + 7;
pub(super) const RELOAD_CONFIG: u32 = WM_APP + 8;
const TRAY_CONFIG: usize = 1;
const TRAY_PAUSE: usize = 2;
const TRAY_QUIT: usize = 3;
fn pack_point(p: POINT) -> LPARAM {
    ((p.x as u32 as u64) | ((p.y as u32 as u64) << 32)) as LPARAM
}
fn unpack_point(l: LPARAM) -> POINT {
    POINT {
        x: l as u32 as i32,
        y: ((l as u64) >> 32) as u32 as i32,
    }
}
unsafe fn pointer_packet() -> LPARAM {
    let mut p: POINT = zeroed();
    GetCursorPos(&mut p);
    pack_point(p)
}

struct Runtime {
    hwnd: HWND,
    root: PathBuf,
    config: Config,
    bindings: Vec<Binding>,
    registered: Vec<bool>,
    engine: Engine,
    keys: [bool; 256],
    eaten: [bool; 256],
    mouse_eaten: [bool; 6],
    pending: Option<usize>,
    paused: bool,
    origin: Point,
    scale: f64,
    saved: POINT,
    keyboard: HHOOK,
    mouse: HHOOK,
    foreground_hook: HWINEVENTHOOK,
    location_hook: HWINEVENTHOOK,
    chimes: Chimes,
    foreground_pending: bool,
    smoke: bool,
    smoke_phase: usize,
    smoke_opened: bool,
    instance: HANDLE,
    reload_requested: bool,
}
static mut APP: *mut Runtime = null_mut();

pub fn run(root: PathBuf, smoke: bool) -> Result<(), String> {
    unsafe {
        let instance = CreateMutexW(
            null(),
            1,
            wide(if smoke {
                "Local\\ConstellationSmoke"
            } else {
                "Local\\ConstellationResident"
            })
            .as_ptr(),
        );
        if instance.is_null() {
            return Err("Cannot create the application instance guard".into());
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(instance);
            return Err("Constellation is already running in the tray.".into());
        }
        let config = storage::load_or_create(&root)?;
        let module = GetModuleHandleW(null());
        let class = wide("Constellation.Native.Radial");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: module,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            ..zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            return Err("Cannot register radial window class".into());
        }
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class.as_ptr(),
            wide("Constellation").as_ptr(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            null_mut(),
            null_mut(),
            module,
            null(),
        );
        if hwnd.is_null() {
            return Err("Cannot create native overlay".into());
        }
        SetLayeredWindowAttributes(hwnd, 0, 255, LWA_COLORKEY);
        let bindings = config
            .profiles
            .iter()
            .map(|p| Binding::parse(&p.binding))
            .collect::<Result<_, _>>()?;
        let app = Box::new(Runtime {
            hwnd,
            root,
            config,
            bindings,
            registered: vec![],
            engine: Engine::default(),
            keys: [false; 256],
            eaten: [false; 256],
            mouse_eaten: [false; 6],
            pending: None,
            paused: false,
            origin: Point::default(),
            scale: 1.,
            saved: zeroed(),
            keyboard: null_mut(),
            mouse: null_mut(),
            foreground_hook: null_mut(),
            location_hook: null_mut(),
            chimes: Chimes::default(),
            foreground_pending: false,
            smoke,
            smoke_phase: 0,
            smoke_opened: false,
            instance,
            reload_requested: false,
        });
        APP = Box::into_raw(app);
        let a = &mut *APP;
        for vk in 0..256 {
            a.keys[vk] = (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0;
        }
        a.keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module, 0);
        a.mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0);
        if a.keyboard.is_null() || a.mouse.is_null() {
            cleanup();
            return Err("Windows input hooks could not be installed".into());
        }
        a.foreground_hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            null_mut(),
            Some(foreground_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        a.location_hook = SetWinEventHook(
            EVENT_OBJECT_LOCATIONCHANGE,
            EVENT_OBJECT_LOCATIONCHANGE,
            null_mut(),
            Some(foreground_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        let mut tray: NOTIFYICONDATAW = zeroed();
        tray.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        tray.hWnd = hwnd;
        tray.uID = 1;
        tray.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        tray.uCallbackMessage = TRAY;
        tray.hIcon = LoadIconW(null_mut(), IDI_APPLICATION);
        let tip = wide("Constellation — right-click for settings");
        tray.szTip[..tip.len()].copy_from_slice(&tip);
        if Shell_NotifyIconW(NIM_ADD, &tray) == 0 {
            cleanup();
            return Err("Could not add Constellation to the notification area".into());
        }
        a.refresh_registration();
        if smoke {
            SetTimer(hwnd, 10, 350, None);
        }
        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let mut tray: NOTIFYICONDATAW = zeroed();
        tray.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        tray.hWnd = hwnd;
        tray.uID = 1;
        Shell_NotifyIconW(NIM_DELETE, &tray);
        cleanup();
        Ok(())
    }
}

unsafe fn cleanup() {
    if APP.is_null() {
        return;
    }
    let mut a = Box::from_raw(APP);
    APP = null_mut();
    a.dismiss(true);
    for i in 0..a.registered.len() {
        UnregisterHotKey(a.hwnd, i as i32 + 100);
    }
    if !a.keyboard.is_null() {
        UnhookWindowsHookEx(a.keyboard);
    }
    if !a.mouse.is_null() {
        UnhookWindowsHookEx(a.mouse);
    }
    if !a.foreground_hook.is_null() {
        UnhookWinEvent(a.foreground_hook);
    }
    if !a.location_hook.is_null() {
        UnhookWinEvent(a.location_hook);
    }
    DestroyWindow(a.hwnd);
    ReleaseMutex(a.instance);
    CloseHandle(a.instance);
}

/// Desktop shell windows cover the screen but are not fullscreen applications.
unsafe fn fullscreen(overlay: HWND) -> bool {
    let hwnd = GetForegroundWindow();
    if hwnd.is_null() || hwnd == overlay || IsIconic(hwnd) != 0 {
        return false;
    }
    if IsZoomed(hwnd) != 0 && (GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_CAPTION) == WS_CAPTION
    {
        return false;
    }
    let mut class = [0u16; 128];
    let n = GetClassNameW(hwnd, class.as_mut_ptr(), 128);
    let class = String::from_utf16_lossy(&class[..n as usize]);
    if matches!(
        class.as_str(),
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
    ) {
        return false;
    }
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut info: MONITORINFO = zeroed();
    info.cbSize = size_of::<MONITORINFO>() as u32;
    let mut r: RECT = zeroed();
    if GetMonitorInfoW(monitor, &mut info) == 0 || GetWindowRect(hwnd, &mut r) == 0 {
        return false;
    }
    let m = info.rcMonitor;
    r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
}

impl Runtime {
    unsafe fn refresh_registration(&mut self) {
        for i in 0..self.registered.len() {
            if self.registered[i] {
                UnregisterHotKey(self.hwnd, i as i32 + 100);
            }
        }
        self.registered = vec![false; self.bindings.len()];
        if self.paused || fullscreen(self.hwnd) {
            return;
        }
        let mut failures = vec![];
        for (i, b) in self.bindings.iter().enumerate() {
            if let Input::Key(vk) = b.key {
                let modifiers = (if b.ctrl { MOD_CONTROL } else { 0 })
                    | (if b.shift { MOD_SHIFT } else { 0 })
                    | (if b.alt { MOD_ALT } else { 0 })
                    | MOD_NOREPEAT;
                if RegisterHotKey(self.hwnd, i as i32 + 100, modifiers, vk as u32) != 0 {
                    self.registered[i] = true
                } else {
                    failures.push(self.config.profiles[i].binding.clone());
                }
            } else {
                self.registered[i] = true;
            }
        }
        if !failures.is_empty() {
            self.notice(&format!(
                "Unavailable global bindings: {}. Other profiles remain usable.",
                failures.join(", ")
            ));
        }
    }
    unsafe fn notice(&self, s: &str) {
        let _ = std::fs::write(self.root.join("data/last-error.txt"), s);
        let mut tray: NOTIFYICONDATAW = zeroed();
        tray.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        tray.hWnd = self.hwnd;
        tray.uID = 1;
        tray.uFlags = NIF_INFO;
        for (to, from) in tray.szInfo.iter_mut().zip(wide(s).into_iter().take(255)) {
            *to = from;
        }
        for (to, from) in tray.szInfoTitle.iter_mut().zip(wide("Constellation")) {
            *to = from;
        }
        tray.dwInfoFlags = NIIF_ERROR;
        Shell_NotifyIconW(NIM_MODIFY, &tray);
    }
    unsafe fn reload(&mut self) {
        match storage::read(&self.root) {
            Ok(c) => {
                self.bindings = c
                    .profiles
                    .iter()
                    .map(|p| Binding::parse(&p.binding).expect("validated binding"))
                    .collect();
                self.config = c;
                self.refresh_registration();
                self.reload_requested = false;
            }
            Err(e) => self.notice(&e),
        }
    }
    unsafe fn open(&mut self, mut i: usize) {
        self.pending = None;
        if self.paused || fullscreen(self.hwnd) {
            self.refresh_registration();
            return;
        }
        if let Some(active) = self.engine.profile {
            if active == i && self.config.profiles[i].mode == Mode::Click {
                self.dismiss(true);
            }
            return;
        }
        let invoked = self.bindings.get(i).cloned();
        match storage::read(&self.root) {
            Ok(c) => {
                let new_bindings = c
                    .profiles
                    .iter()
                    .map(|p| Binding::parse(&p.binding))
                    .collect::<Result<Vec<_>, _>>();
                if let Ok(b) = new_bindings {
                    let changed = b != self.bindings;
                    self.config = c;
                    if changed {
                        self.bindings = b;
                        self.refresh_registration();
                        let Some(index) = self
                            .bindings
                            .iter()
                            .position(|b| Some(b) == invoked.as_ref())
                        else {
                            return;
                        };
                        i = index;
                    }
                }
            }
            Err(e) => self.notice(&e),
        }
        if !self.engine.open(i, &self.config) {
            return;
        }
        if GetCursorPos(&mut self.saved) == 0 {
            self.engine.close();
            self.notice("Cannot read cursor position; invocation cancelled.");
            return;
        }
        let monitor = MonitorFromPoint(self.saved, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            self.engine.close();
            return;
        }
        let r = info.rcWork;
        self.origin = Point {
            x: (r.right - r.left) as f64 / 2.,
            y: (r.bottom - r.top) as f64 / 2.,
        };
        SetWindowPos(
            self.hwnd,
            HWND_TOPMOST,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            SWP_NOACTIVATE,
        );
        self.scale = GetDpiForWindow(self.hwnd) as f64 / 96.;
        let x = r.left + self.origin.x as i32;
        let y = r.top + self.origin.y as i32;
        if SetCursorPos(x, y) == 0 {
            self.engine.close();
            self.notice("Cannot center the cursor; invocation cancelled.");
            return;
        }
        let mut actual: POINT = zeroed();
        GetCursorPos(&mut actual);
        if actual.x != x || actual.y != y {
            self.engine.close();
            SetCursorPos(self.saved.x, self.saved.y);
            self.notice("Another application confines the cursor; invocation cancelled.");
            return;
        }
        ShowWindow(self.hwnd, SW_SHOW);
        SetForegroundWindow(self.hwnd);
        SetFocus(self.hwnd);
        InvalidateRect(self.hwnd, null(), 0);
    }
    unsafe fn dismiss(&mut self, restore: bool) {
        let should = self
            .engine
            .profile
            .is_some_and(|i| self.config.profiles[i].restore_cursor)
            && restore;
        self.engine.close();
        self.pending = None;
        ShowWindow(self.hwnd, SW_HIDE);
        if should {
            SetCursorPos(self.saved.x, self.saved.y);
        }
        if self.reload_requested {
            self.reload();
        }
    }
    unsafe fn pointer_at(&mut self, mut p: POINT) {
        if self.engine.profile.is_none() {
            return;
        }
        ScreenToClient(self.hwnd, &mut p);
        let action = self.engine.motion(
            &self.config,
            Point {
                x: (p.x as f64 - self.origin.x) / self.scale,
                y: (p.y as f64 - self.origin.y) / self.scale,
            },
        );
        if action != Action::None {
            if self.engine.hovered.is_some() {
                self.chimes.play(self.config.sound);
            }
            InvalidateRect(self.hwnd, null(), 0);
        }
    }
    unsafe fn action(&mut self, a: Action) {
        match a {
            Action::Launch(id) => {
                let t = self.config.find(id).and_then(|n| n.target.clone());
                self.dismiss(!fullscreen(self.hwnd));
                if let Some(t) = t {
                    if let Err(e) = target::launch(&self.root, &t) {
                        self.notice(&e);
                    }
                }
            }
            Action::Changed => {
                if self.engine.profile.is_none() {
                    ShowWindow(self.hwnd, SW_HIDE);
                }
                InvalidateRect(self.hwnd, null(), 0);
            }
            Action::None => {}
        }
    }
    unsafe fn configure(&mut self) {
        self.dismiss(true);
        if let Err(e) = editor::spawn() {
            self.notice(&e);
        }
    }
}

unsafe extern "system" fn keyboard_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code < 0 || APP.is_null() {
        return CallNextHookEx(null_mut(), code, w, l);
    }
    let Some(_guard) = super::DispatchGuard::enter() else {
        return CallNextHookEx(null_mut(), code, w, l);
    };
    let a = &mut *APP;
    let e = &*(l as *const KBDLLHOOKSTRUCT);
    let vk = e.vkCode as usize;
    if vk >= 256 {
        return CallNextHookEx(null_mut(), code, w, l);
    }
    let down = matches!(w as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
    let up = matches!(w as u32, WM_KEYUP | WM_SYSKEYUP);
    if !down && !up {
        return CallNextHookEx(null_mut(), code, w, l);
    }
    let repeat = down && a.keys[vk];
    a.keys[vk] = down;
    if matches!(vk, 0xa2 | 0xa3) {
        a.keys[0x11] = a.keys[0xa2] || a.keys[0xa3];
    }
    if matches!(vk, 0xa0 | 0xa1) {
        a.keys[0x10] = a.keys[0xa0] || a.keys[0xa1];
    }
    if matches!(vk, 0xa4 | 0xa5) {
        a.keys[0x12] = a.keys[0xa4] || a.keys[0xa5];
    }
    let active = a.engine.profile.or(a.pending);
    if up {
        if let Some(i) = active {
            if a.config
                .profiles
                .get(i)
                .is_some_and(|p| p.mode == Mode::Hold)
                && a.bindings[i].requires(Input::Key(vk as u16))
            {
                PostMessageW(a.hwnd, RELEASE, i, pointer_packet());
            }
        }
        let consumed = a.eaten[vk];
        a.eaten[vk] = false;
        return if consumed {
            1
        } else {
            CallNextHookEx(null_mut(), code, w, l)
        };
    }
    if a.eaten[vk] {
        return 1;
    }
    let ctrl = a.keys[0x11];
    let shift = a.keys[0x10];
    let alt = a.keys[0x12];
    let profile = a
        .bindings
        .iter()
        .enumerate()
        .find(|(i, b)| {
            a.registered.get(*i) == Some(&true)
                && b.key == Input::Key(vk as u16)
                && b.ctrl == ctrl
                && b.shift == shift
                && b.alt == alt
                && !a.keys[0x5b]
                && !a.keys[0x5c]
        })
        .map(|(i, _)| i);
    if let Some(i) = profile {
        if a.paused || fullscreen(a.hwnd) {
            a.refresh_registration();
            if active.is_some() {
                PostMessageW(a.hwnd, FOREGROUND, 0, 0);
            }
            return CallNextHookEx(null_mut(), code, w, l);
        }
        a.eaten[vk] = true;
        if !repeat {
            if active.is_none() {
                a.pending = Some(i);
                PostMessageW(a.hwnd, INVOKE, i, 0);
            } else if active == Some(i) && a.config.profiles[i].mode == Mode::Click {
                PostMessageW(a.hwnd, INVOKE, i, 0);
            }
        }
        return 1;
    }
    if active.is_some() && !repeat && matches!(vk, 0x1b | 0x20 | 0x08) {
        a.eaten[vk] = true;
        PostMessageW(a.hwnd, KEY_ACTION, vk, pointer_packet());
        return 1;
    }
    CallNextHookEx(null_mut(), code, w, l)
}

unsafe extern "system" fn mouse_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code < 0 || APP.is_null() {
        return CallNextHookEx(null_mut(), code, w, l);
    }
    let Some(_guard) = super::DispatchGuard::enter() else {
        return CallNextHookEx(null_mut(), code, w, l);
    };
    let a = &mut *APP;
    let e = &*(l as *const MSLLHOOKSTRUCT);
    let msg = w as u32;
    if msg == WM_MOUSEMOVE {
        if a.engine.profile.is_some() {
            PostMessageW(a.hwnd, POINTER_MOVE, 0, pack_point(e.pt));
        }
        return CallNextHookEx(null_mut(), code, w, l);
    }
    let (button, down) = match msg {
        WM_LBUTTONDOWN => (1, true),
        WM_LBUTTONUP => (1, false),
        WM_RBUTTONDOWN => (2, true),
        WM_RBUTTONUP => (2, false),
        WM_MBUTTONDOWN => (3, true),
        WM_MBUTTONUP => (3, false),
        WM_XBUTTONDOWN | WM_XBUTTONUP => (
            (if e.mouseData >> 16 == 1 { 4 } else { 5 }),
            msg == WM_XBUTTONDOWN,
        ),
        _ => return CallNextHookEx(null_mut(), code, w, l),
    };
    let active = a.engine.profile.or(a.pending);
    if !down {
        if let Some(i) = active {
            if a.config.profiles[i].mode == Mode::Hold && a.bindings[i].key == Input::Mouse(button)
            {
                PostMessageW(a.hwnd, RELEASE, i, pack_point(e.pt));
            }
        }
        let consumed = a.mouse_eaten[button as usize];
        a.mouse_eaten[button as usize] = false;
        return if consumed {
            1
        } else {
            CallNextHookEx(null_mut(), code, w, l)
        };
    }
    if a.paused || fullscreen(a.hwnd) {
        if active.is_some() {
            PostMessageW(a.hwnd, FOREGROUND, 0, 0);
        }
        return CallNextHookEx(null_mut(), code, w, l);
    }
    if let Some(i) = a
        .bindings
        .iter()
        .enumerate()
        .find(|(_, b)| b.key == Input::Mouse(button))
        .map(|(i, _)| i)
    {
        a.mouse_eaten[button as usize] = true;
        if active.is_none() {
            a.pending = Some(i);
            PostMessageW(a.hwnd, INVOKE, i, 0);
        } else if active == Some(i) && a.config.profiles[i].mode == Mode::Click {
            PostMessageW(a.hwnd, INVOKE, i, 0);
        }
        return 1;
    }
    if active.is_some() && matches!(button, 1 | 2) {
        a.mouse_eaten[button as usize] = true;
        PostMessageW(a.hwnd, CLICK, button as usize, pack_point(e.pt));
        return 1;
    }
    CallNextHookEx(null_mut(), code, w, l)
}

unsafe extern "system" fn foreground_event(
    _: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    if APP.is_null() {
        return;
    }
    let Some(_guard) = super::DispatchGuard::enter() else {
        PostMessageW((*APP).hwnd, FOREGROUND, 0, 0);
        return;
    };
    let a = &mut *APP;
    if event == EVENT_OBJECT_LOCATIONCHANGE && (hwnd != GetForegroundWindow() || object != 0) {
        return;
    }
    if !a.foreground_pending {
        a.foreground_pending = true;
        PostMessageW(a.hwnd, FOREGROUND, 0, 0);
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if APP.is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let Some(_guard) = super::DispatchGuard::enter() else {
        return DefWindowProcW(hwnd, msg, w, l);
    };
    let a = &mut *APP;
    match msg {
        RELOAD_CONFIG => {
            if a.engine.profile.is_some() {
                a.reload_requested = true;
            } else {
                a.reload();
            }
            0
        }
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            render::buffered(hwnd, 0, |dc| {
                render::draw(
                    dc,
                    &a.config,
                    &a.engine.sectors,
                    &a.engine.path,
                    a.engine.hovered,
                    a.origin,
                    a.scale,
                    false,
                )
            });
            0
        }
        INVOKE => {
            a.open(w);
            0
        }
        POINTER_MOVE => {
            a.pointer_at(unpack_point(l));
            0
        }
        WM_MOUSEMOVE => {
            // Cursor warps and accessibility tools can generate window mouse messages
            // without a low-level hook event. Preserve that route as well.
            let mut p = POINT {
                x: l as u16 as i16 as i32,
                y: ((l as u32) >> 16) as u16 as i16 as i32,
            };
            ClientToScreen(hwnd, &mut p);
            a.pointer_at(p);
            0
        }
        RELEASE => {
            if a.engine.profile == Some(w) {
                a.pointer_at(unpack_point(l));
                let restore = a.config.profiles[w].restore_cursor;
                let result = a.engine.release(&a.config);
                if a.engine.profile.is_none() {
                    ShowWindow(hwnd, SW_HIDE);
                    if restore && !fullscreen(hwnd) {
                        SetCursorPos(a.saved.x, a.saved.y);
                    }
                    if a.reload_requested {
                        a.reload();
                    }
                } else {
                    a.action(result);
                }
            }
            0
        }
        KEY_ACTION => {
            if a.engine.profile.is_none() {
                return 0;
            }
            match w {
                0x1b => a.dismiss(true),
                0x20 => {
                    a.pointer_at(unpack_point(l));
                    if a.engine
                        .hovered
                        .is_some_and(|id| a.config.find(id).is_some_and(|n| n.target.is_some()))
                    {
                        let action = a.engine.select(&a.config);
                        a.action(action);
                    }
                }
                0x08 => {
                    a.engine.back(&a.config);
                    InvalidateRect(hwnd, null(), 0);
                }
                _ => {}
            }
            0
        }
        CLICK => {
            if a.engine.profile.is_none() {
                return 0;
            }
            a.pointer_at(unpack_point(l));
            if w == 2 {
                a.dismiss(true);
            } else {
                let mut p = unpack_point(l);
                ScreenToClient(hwnd, &mut p);
                let r = ((p.x as f64 - a.origin.x) / a.scale)
                    .hypot((p.y as f64 - a.origin.y) / a.scale);
                if r < a.config.layout.radius * 0.27 {
                    a.engine.root(&a.config);
                    InvalidateRect(hwnd, null(), 0);
                } else if a.engine.hovered.is_none() {
                    a.dismiss(true);
                } else {
                    let action = a.engine.select(&a.config);
                    a.action(action);
                }
            }
            0
        }
        FOREGROUND => {
            a.foreground_pending = false;
            let full = fullscreen(hwnd);
            if a.engine.profile.is_some() && (full || GetForegroundWindow() != hwnd) {
                a.dismiss(!full);
            }
            if a.engine.profile.is_none() {
                a.refresh_registration();
            }
            0
        }
        WM_ACTIVATE => {
            if w & 0xffff == WA_INACTIVE as usize && a.engine.profile.is_some() {
                PostMessageW(hwnd, FOREGROUND, 0, 0);
            }
            0
        }
        WM_DISPLAYCHANGE => {
            a.dismiss(!fullscreen(hwnd));
            a.refresh_registration();
            0
        }
        WM_DPICHANGED => {
            a.dismiss(!fullscreen(hwnd));
            0
        }
        TRAY => {
            match l as u32 {
                WM_LBUTTONDBLCLK => a.configure(),
                WM_RBUTTONUP | WM_CONTEXTMENU => {
                    let menu = CreatePopupMenu();
                    AppendMenuW(
                        menu,
                        MF_STRING,
                        TRAY_CONFIG,
                        wide("Configure / Visualize…").as_ptr(),
                    );
                    AppendMenuW(
                        menu,
                        MF_STRING | if a.paused { MF_CHECKED } else { 0 },
                        TRAY_PAUSE,
                        wide("Pause shortcuts").as_ptr(),
                    );
                    AppendMenuW(menu, MF_SEPARATOR, 0, null());
                    AppendMenuW(menu, MF_STRING, TRAY_QUIT, wide("Quit").as_ptr());
                    let mut p: POINT = zeroed();
                    GetCursorPos(&mut p);
                    SetForegroundWindow(hwnd);
                    let command = TrackPopupMenu(
                        menu,
                        TPM_RETURNCMD | TPM_RIGHTBUTTON,
                        p.x,
                        p.y,
                        0,
                        hwnd,
                        null(),
                    );
                    DestroyMenu(menu);
                    match command as usize {
                        TRAY_CONFIG => a.configure(),
                        TRAY_PAUSE => {
                            a.paused = !a.paused;
                            a.dismiss(true);
                            a.refresh_registration();
                        }
                        TRAY_QUIT => PostQuitMessage(0),
                        _ => {}
                    }
                }
                _ => {}
            }
            0
        }
        WM_TIMER if a.smoke => {
            a.smoke_phase += 1;
            match a.smoke_phase {
                1 => {
                    a.open(0);
                    a.smoke_opened = a.engine.profile.is_some();
                }
                2 => {
                    if let Some(s) = a
                        .engine
                        .sectors
                        .iter()
                        .find(|s| a.config.find(s.id).is_some_and(|n| n.children.is_some()))
                        .cloned()
                    {
                        a.engine.motion(&a.config, s.dot);
                        InvalidateRect(hwnd, null(), 0);
                    }
                }
                3 => {
                    a.dismiss(true);
                }
                4 => {
                    let mut cursor: POINT = zeroed();
                    GetCursorPos(&mut cursor);
                    let report = format!(
                        "overlay_opened={}\ncursor_restored={}\nkeyboard_hook={}\nmouse_hook={}\n",
                        a.smoke_opened,
                        cursor.x == a.saved.x && cursor.y == a.saved.y,
                        !a.keyboard.is_null(),
                        !a.mouse.is_null()
                    );
                    let _ = std::fs::write(a.root.join("data/smoke-test.txt"), report);
                    PostQuitMessage(0);
                }
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            a.dismiss(true);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}
