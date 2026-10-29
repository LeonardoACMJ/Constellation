#![allow(unsafe_op_in_unsafe_fn)]
use super::{audio::Chimes, config_reload, editor, input::Worker, smooth, target, wide};
use crate::{
    config::{Binding, Config, CursorEnd, Mode},
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
    System::{LibraryLoader::GetModuleHandleW, RemoteDesktop::*, Threading::*},
    UI::{HiDpi::*, Shell::*, WindowsAndMessaging::*},
};

const RADIAL_WINDOW_STYLE: u32 =
    WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE;
const TRAY: u32 = WM_APP + 1;
pub(super) const INVOKE: u32 = WM_APP + 2;
pub(super) const RELEASE: u32 = WM_APP + 3;
pub(super) const KEY_ACTION: u32 = WM_APP + 4;
pub(super) const FOREGROUND: u32 = WM_APP + 5;
pub(super) const POINTER_MOVE: u32 = WM_APP + 6;
pub(super) const CLICK: u32 = WM_APP + 7;
pub(super) const RELOAD_CONFIG: u32 = WM_APP + 8;
const TRAY_MENU_DONE: u32 = WM_APP + 9;
const DEFERRED_DPI_CHANGE: u32 = WM_APP + 10;
const DEFERRED_POWER_CHANGE: u32 = WM_APP + 11;
const TRAY_CONFIG: usize = 1;
const TRAY_PAUSE: usize = 2;
const TRAY_QUIT: usize = 3;
pub(super) fn pack_point(p: POINT) -> LPARAM {
    ((p.x as u32 as u64) | ((p.y as u32 as u64) << 32)) as LPARAM
}
fn unpack_point(l: LPARAM) -> POINT {
    POINT {
        x: l as u32 as i32,
        y: ((l as u64) >> 32) as u32 as i32,
    }
}
pub(super) unsafe fn pointer_packet() -> LPARAM {
    let mut p: POINT = zeroed();
    GetCursorPos(&mut p);
    pack_point(p)
}
fn center_in_clip(center: POINT, clip: RECT) -> bool {
    center.x >= clip.left && center.x < clip.right && center.y >= clip.top && center.y < clip.bottom
}

struct Runtime {
    hwnd: HWND,
    root: PathBuf,
    config: Config,
    bindings: Vec<Binding>,
    engine: Engine,
    paused: bool,
    menu_open: bool,
    source_foreground: HWND,
    renderer: smooth::Renderer,
    launches: target::Dispatcher,
    cursor: super::cursor::DotCursor,
    session_blocked: bool,
    sleeping: bool,
    session_notifications: bool,
    taskbar_created: u32,
    origin: Point,
    scale: f64,
    saved: POINT,
    center: POINT,
    input: Option<Worker>,
    chimes: Chimes,
    smoke: bool,
    smoke_phase: usize,
    smoke_opened: bool,
    instance: HANDLE,
    reload: Option<config_reload::Worker>,
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
            hIcon: super::icons::large(),
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
            RADIAL_WINDOW_STYLE,
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
        super::icons::set_small(hwnd);
        if hwnd.is_null() {
            return Err("Cannot create native overlay".into());
        }
        let bindings = config
            .profiles
            .iter()
            .map(|p| Binding::parse(&p.binding))
            .collect::<Result<_, _>>()?;
        let app = Box::new(Runtime::new(hwnd, root, config, bindings, smoke, instance));
        APP = Box::into_raw(app);
        let startup_guard = super::DispatchGuard::enter().expect("resident startup dispatch");
        let a = &mut *APP;
        match config_reload::Worker::start(hwnd, a.root.clone()) {
            Ok(worker) => a.reload = Some(worker),
            Err(error) => {
                cleanup();
                return Err(error);
            }
        }
        match Worker::start(
            hwnd,
            a.bindings.clone(),
            a.config.profiles.iter().map(|p| p.mode).collect(),
            a.root.clone(),
        ) {
            Ok(input) => a.input = Some(input),
            Err(e) => {
                cleanup();
                return Err(e);
            }
        }
        if !add_tray(hwnd) {
            cleanup();
            return Err("Could not add Constellation to the notification area".into());
        }
        if let Some(message) = storage::recovery_notice(&a.root) {
            a.notice_kind(&message, NIIF_INFO);
        }
        a.session_notifications =
            WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) != 0;
        if !a.session_notifications {
            cleanup();
            return Err("Cannot subscribe to Windows session changes".into());
        }
        a.refresh_registration();
        if smoke {
            SetTimer(hwnd, 10, 350, None);
        }
        drop(startup_guard);
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

unsafe fn add_tray(hwnd: HWND) -> bool {
    let mut tray: NOTIFYICONDATAW = zeroed();
    tray.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    tray.hWnd = hwnd;
    tray.uID = 1;
    tray.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    tray.uCallbackMessage = TRAY;
    tray.hIcon = super::icons::small();
    let tip = wide("Constellation — right-click for settings");
    tray.szTip[..tip.len()].copy_from_slice(&tip);
    Shell_NotifyIconW(NIM_ADD, &tray) != 0 || Shell_NotifyIconW(NIM_MODIFY, &tray) != 0
}

unsafe fn cleanup() {
    if APP.is_null() {
        return;
    }
    let mut a = Box::from_raw(APP);
    APP = null_mut();
    drop(a.reload.take());
    a.dismiss(true);
    drop(a.input.take());
    if a.session_notifications {
        WTSUnRegisterSessionNotification(a.hwnd);
    }
    DestroyWindow(a.hwnd);
    ReleaseMutex(a.instance);
    CloseHandle(a.instance);
}

/// Yield input to fullscreen applications and the shortcut-profile editor.
pub(super) unsafe fn foreground_blocks_launcher(overlay: HWND) -> bool {
    let hwnd = GetForegroundWindow();
    if hwnd.is_null() || hwnd == overlay || IsIconic(hwnd) != 0 {
        return false;
    }
    let mut class = [0u16; 128];
    let n = GetClassNameW(hwnd, class.as_mut_ptr(), 128);
    let class = String::from_utf16_lossy(&class[..n as usize]);
    if class == "Constellation.Profiles" {
        return true;
    }
    if IsZoomed(hwnd) != 0 && (GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_CAPTION) == WS_CAPTION
    {
        return false;
    }
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
    fn new(
        hwnd: HWND,
        root: PathBuf,
        config: Config,
        bindings: Vec<Binding>,
        smoke: bool,
        instance: HANDLE,
    ) -> Self {
        unsafe {
            Self {
                hwnd,
                launches: target::Dispatcher::new(hwnd, root.clone()),
                root,
                config,
                bindings,
                engine: Engine::default(),
                paused: false,
                menu_open: false,
                source_foreground: null_mut(),
                renderer: smooth::Renderer::default(),
                cursor: super::cursor::DotCursor::default(),
                session_blocked: false,
                sleeping: false,
                session_notifications: false,
                taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
                origin: Point::default(),
                scale: 1.,
                saved: zeroed(),
                center: zeroed(),
                input: None,
                chimes: Chimes::default(),
                smoke,
                smoke_phase: 0,
                smoke_opened: false,
                instance,
                reload: None,
            }
        }
    }
    unsafe fn refresh_registration(&mut self) {
        if let Some(input) = &self.input {
            input.configure(
                self.bindings.clone(),
                self.config.profiles.iter().map(|p| p.mode).collect(),
                !self.paused && !self.session_blocked && !self.sleeping && !self.menu_open,
            );
        }
    }
    unsafe fn notice(&self, s: &str) {
        self.notice_kind(s, NIIF_ERROR);
    }
    unsafe fn notice_kind(&self, s: &str, flags: u32) {
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
        tray.dwInfoFlags = flags;
        Shell_NotifyIconW(NIM_MODIFY, &tray);
    }
    unsafe fn apply_ready_config(&mut self) {
        let Some(result) = self
            .reload
            .as_ref()
            .and_then(|worker| worker.take_ready(self.engine.profile.is_some()))
        else {
            return;
        };
        match result {
            Ok(snapshot) => {
                // Saving unchanged JSON is also an explicit icon refresh.
                self.renderer.invalidate_icons();
                if snapshot.config == self.config {
                    return;
                }
                // Quiesce old bindings before draining queued gesture indices.
                // A queued old index must never dispatch a different new profile.
                if let Some(input) = &self.input {
                    input.configure(
                        self.bindings.clone(),
                        self.config.profiles.iter().map(|p| p.mode).collect(),
                        false,
                    );
                }
                self.cancel_packets();
                self.config = snapshot.config;
                self.bindings = snapshot.bindings;
                self.resync_input();
                self.refresh_registration();
            }
            Err(error) => self.notice(&error),
        }
    }
    unsafe fn open(&mut self, i: usize) {
        self.open_inner(i);
        if let Some(input) = &self.input {
            input.active(self.engine.profile);
        }
    }
    unsafe fn open_inner(&mut self, i: usize) {
        if self.paused
            || self.menu_open
            || self.session_blocked
            || self.sleeping
            || foreground_blocks_launcher(self.hwnd)
        {
            self.refresh_registration();
            return;
        }
        if let Some(active) = self.engine.profile {
            if active == i && self.config.profiles[i].mode == Mode::Click {
                self.dismiss(true);
            }
            return;
        }
        if !self.engine.open(i, &self.config) {
            return;
        }
        self.source_foreground = GetForegroundWindow();
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
        self.center = POINT { x, y };
        let mut clip: RECT = zeroed();
        if GetClipCursor(&mut clip) != 0 && !center_in_clip(self.center, clip) {
            self.engine.close();
            self.notice("Another application confines the cursor; invocation cancelled.");
            return;
        }
        if SetCursorPos(x, y) == 0 {
            self.engine.close();
            self.notice("Cannot center the cursor; invocation cancelled.");
            return;
        }
        // Physical motion may immediately move away from the centre. It is
        // not evidence of confinement; inspect the actual clip rectangle above.
        if self.config.profiles[i].dot_cursor {
            self.cursor.prepare(self.scale);
        }
        // Submit the fresh root frame while hidden. Per-pixel alpha keeps
        // curved edges smooth without exposing last invocation's cached pixels.
        if let Err(error) = self.paint_radial() {
            self.dismiss(true);
            self.notice(&error);
            return;
        }
        ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        self.cursor.apply();
    }
    unsafe fn paint_radial(&mut self) -> Result<(), String> {
        let Some(start) = self.engine.entry(&self.config) else {
            return Ok(());
        };
        self.renderer.present(
            self.hwnd,
            &self.config,
            start,
            &self.engine.sectors,
            &self.engine.path,
            self.engine.hovered,
            self.origin,
            self.scale,
            self.cursor.enabled,
            &self.root,
        )
    }
    unsafe fn finish_cursor(&self, policy: CursorEnd) {
        let p = match policy {
            CursorEnd::Restore => self.saved,
            CursorEnd::Center => self.center,
            CursorEnd::Leave => return,
        };
        SetCursorPos(p.x, p.y);
    }
    unsafe fn dismiss(&mut self, restore: bool) {
        let policy = self
            .engine
            .profile
            .map(|i| self.config.profiles[i].cursor_end);
        self.engine.close();
        self.cancel_packets();
        ShowWindow(self.hwnd, SW_HIDE);
        self.cursor.restore();
        self.renderer.release_frame();
        if let Some(policy) = policy.filter(|_| restore) {
            self.finish_cursor(policy);
        }
        self.apply_ready_config();
    }
    unsafe fn cancel_packets(&mut self) {
        if let Some(input) = &self.input {
            input.active(None);
        }
        // Drop queued invocation/selection packets from the cancelled gesture.
        // In particular, a late Hold release must never select from a later menu.
        discard_gesture_packets(self.hwnd);
    }
    unsafe fn pointer_at(&mut self, mut p: POINT) {
        if self.engine.profile.is_none() {
            return;
        }
        ScreenToClient(self.hwnd, &mut p);
        let (action, new_highlight) = self.engine.motion_with_feedback(
            &self.config,
            Point {
                x: (p.x as f64 - self.origin.x) / self.scale,
                y: (p.y as f64 - self.origin.y) / self.scale,
            },
        );
        if action != Action::None {
            if new_highlight {
                self.chimes.play(self.config.sound);
            }
            InvalidateRect(self.hwnd, null(), 0);
        }
    }
    unsafe fn action(&mut self, a: Action) {
        match a {
            Action::Launch(id) => {
                let t = self.config.launch_target(id);
                self.dismiss(!foreground_blocks_launcher(self.hwnd));
                if let Some(t) = t {
                    if let Err(e) = self.launches.launch(t) {
                        self.notice(&e);
                    }
                }
            }
            Action::Changed => {
                if self.engine.profile.is_none() {
                    ShowWindow(self.hwnd, SW_HIDE);
                    self.cursor.restore();
                    self.renderer.release_frame();
                    self.apply_ready_config();
                }
                InvalidateRect(self.hwnd, null(), 0);
            }
            Action::None => {}
        }
    }
    unsafe fn selection_allowed(&mut self) -> bool {
        if self.engine.profile.is_none() {
            return false;
        }
        if self.session_blocked
            || self.sleeping
            || foreground_blocks_launcher(self.hwnd)
            || GetForegroundWindow() != self.source_foreground
        {
            self.dismiss(
                !self.session_blocked && !self.sleeping && !foreground_blocks_launcher(self.hwnd),
            );
            return false;
        }
        true
    }
    unsafe fn resync_input(&mut self) {
        if let Some(input) = &self.input {
            input.resync();
        }
    }
    unsafe fn configure(&mut self) {
        self.dismiss(true);
        if let Err(e) = editor::spawn() {
            self.notice(&e);
        }
    }
}

unsafe fn show_tray_menu(hwnd: HWND, w: WPARAM, l: LPARAM) -> LRESULT {
    let Some(guard) = super::DispatchGuard::enter() else {
        super::defer_message(hwnd, TRAY, w, l);
        return 0;
    };
    if APP.is_null() || (*APP).menu_open {
        return 0;
    }
    let paused = {
        let a = &mut *APP;
        a.menu_open = true;
        a.dismiss(true);
        a.refresh_registration();
        a.paused
    };
    // Consumes/releases the guard before opening the native modal menu.
    let command = track_tray_menu(hwnd, paused, guard);
    PostMessageW(hwnd, TRAY_MENU_DONE, command, 0);
    0
}
unsafe fn track_tray_menu(hwnd: HWND, paused: bool, guard: super::DispatchGuard) -> usize {
    drop(guard);
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return 0;
    }
    AppendMenuW(
        menu,
        MF_STRING,
        TRAY_CONFIG,
        wide("Configure / Visualize…").as_ptr(),
    );
    AppendMenuW(
        menu,
        MF_STRING | if paused { MF_CHECKED } else { 0 },
        TRAY_PAUSE,
        wide("Pause shortcuts").as_ptr(),
    );
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, TRAY_QUIT, wide("Quit").as_ptr());
    let mut point: POINT = zeroed();
    GetCursorPos(&mut point);
    SetForegroundWindow(hwnd);
    let command = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        point.x,
        point.y,
        0,
        hwnd,
        null(),
    );
    DestroyMenu(menu);
    // Standard tray-menu dismissal handshake; no input-queue attachment.
    PostMessageW(hwnd, WM_NULL, 0, 0);
    command as usize
}

unsafe fn defer_runtime_message(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> Option<LRESULT> {
    if super::defer_nested_close(hwnd, msg, w, l) {
        return Some(0);
    }
    if matches!(
        msg,
        WM_WTSSESSION_CHANGE
            | WM_POWERBROADCAST
            | WM_DISPLAYCHANGE
            | WM_DPICHANGED
            | DEFERRED_DPI_CHANGE
            | DEFERRED_POWER_CHANGE
            | WM_CANCELMODE
            | WM_CAPTURECHANGED
            | RELOAD_CONFIG
            | config_reload::READY
            | super::item_icons::READY
            | target::READY
            | TRAY_MENU_DONE
            | FOREGROUND
            | INVOKE
            | RELEASE
            | KEY_ACTION
            | CLICK
            | POINTER_MOVE
            | super::input::ERROR
    ) {
        // Native interruption payloads are not retained; input packets are
        // owned integer coordinates. Never queue an OS-owned RECT pointer.
        let payload = if matches!(msg, WM_DPICHANGED | WM_POWERBROADCAST) {
            0
        } else {
            l
        };
        // Pointer-bearing system notifications cannot be reposted safely.
        // Use private, pointer-free wakeups for their deferred application work.
        let deferred = match msg {
            WM_DPICHANGED => DEFERRED_DPI_CHANGE,
            WM_POWERBROADCAST => DEFERRED_POWER_CHANGE,
            _ => msg,
        };
        super::defer_message(hwnd, deferred, w, payload);
        return Some(if msg == WM_POWERBROADCAST { 1 } else { 0 });
    }
    if msg == WM_QUERYENDSESSION {
        super::defer_message(hwnd, WM_CANCELMODE, 0, 0);
        return Some(1);
    }
    None
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if APP.is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    if msg == WM_SETCURSOR && super::cursor::set_current() {
        return 1;
    }
    if msg == TRAY && matches!(l as u32, WM_RBUTTONUP | WM_CONTEXTMENU) {
        return show_tray_menu(hwnd, w, l);
    }
    let Some(_guard) = super::DispatchGuard::enter() else {
        return defer_runtime_message(hwnd, msg, w, l)
            .unwrap_or_else(|| DefWindowProcW(hwnd, msg, w, l));
    };
    let a = &mut *APP;
    if msg == a.taskbar_created && msg != 0 {
        add_tray(hwnd);
        return 0;
    }
    match msg {
        target::READY => {
            for result in a.launches.results() {
                if let Err(error) = result {
                    a.notice(&error);
                }
            }
            0
        }
        super::item_icons::READY => {
            a.renderer.icons_ready();
            if a.engine.profile.is_some() {
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        super::input::ERROR => {
            let message = Box::from_raw(l as *mut String);
            a.notice(&message);
            0
        }
        RELOAD_CONFIG => {
            if let Some(worker) = &a.reload {
                worker.request();
            }
            0
        }
        config_reload::READY => {
            a.apply_ready_config();
            0
        }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as isize,
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut paint: PAINTSTRUCT = zeroed();
            BeginPaint(hwnd, &mut paint);
            EndPaint(hwnd, &paint);
            if a.engine.profile.is_some() {
                if let Err(error) = a.paint_radial() {
                    a.dismiss(true);
                    a.notice(&error);
                }
            }
            0
        }
        INVOKE => {
            a.open(w);
            0
        }
        POINTER_MOVE | WM_MOUSEMOVE => {
            // Cursor warps and accessibility tools can generate window mouse messages
            // without a low-level hook event. Preserve that route as well.
            // Both routes can retain pre-centering coordinates while opening.
            // Read the current cursor, never replay those absolute positions.
            let mut p: POINT = zeroed();
            if GetCursorPos(&mut p) != 0 {
                a.pointer_at(p);
            }
            0
        }
        RELEASE => {
            if a.selection_allowed() && a.engine.profile == Some(w) {
                a.pointer_at(unpack_point(l));
                let policy = a.config.profiles[w].cursor_end;
                let result = a.engine.release(&a.config);
                if a.engine.profile.is_none() {
                    a.cancel_packets();
                    ShowWindow(hwnd, SW_HIDE);
                    a.cursor.restore();
                    a.renderer.release_frame();
                    if !foreground_blocks_launcher(hwnd) {
                        a.finish_cursor(policy);
                    }
                    a.apply_ready_config();
                } else {
                    a.action(result);
                }
            }
            0
        }
        KEY_ACTION => {
            if !a.selection_allowed() {
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
            if !a.selection_allowed() {
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
            let full = foreground_blocks_launcher(hwnd);
            if a.engine.profile.is_some() && (full || GetForegroundWindow() != a.source_foreground)
            {
                a.dismiss(!full);
            }
            0
        }
        WM_ACTIVATE => {
            if w & 0xffff == WA_INACTIVE as usize && a.engine.profile.is_some() {
                PostMessageW(hwnd, FOREGROUND, 0, 0);
            }
            0
        }
        WM_WTSSESSION_CHANGE => {
            match w as u32 {
                WTS_SESSION_LOCK
                | WTS_CONSOLE_DISCONNECT
                | WTS_REMOTE_DISCONNECT
                | WTS_SESSION_LOGOFF => {
                    a.session_blocked = true;
                    a.dismiss(false);
                }
                WTS_SESSION_UNLOCK | WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT
                | WTS_SESSION_LOGON => {
                    a.session_blocked = false;
                    a.resync_input();
                }
                _ => {}
            }
            a.refresh_registration();
            0
        }
        WM_POWERBROADCAST | DEFERRED_POWER_CHANGE => {
            match w as u32 {
                PBT_APMSUSPEND => {
                    a.sleeping = true;
                    a.dismiss(false);
                }
                PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => {
                    a.sleeping = false;
                    a.resync_input();
                }
                _ => {}
            }
            a.refresh_registration();
            1
        }
        WM_QUERYENDSESSION => {
            a.dismiss(false);
            1
        }
        WM_ENDSESSION if w != 0 => {
            PostQuitMessage(0);
            0
        }
        WM_CANCELMODE | WM_CAPTURECHANGED => {
            a.dismiss(!foreground_blocks_launcher(hwnd));
            0
        }
        WM_DISPLAYCHANGE => {
            a.dismiss(!foreground_blocks_launcher(hwnd));
            a.refresh_registration();
            0
        }
        WM_DPICHANGED | DEFERRED_DPI_CHANGE => {
            a.dismiss(!foreground_blocks_launcher(hwnd));
            0
        }
        TRAY => {
            if l as u32 == WM_LBUTTONDBLCLK && !a.menu_open {
                a.configure();
            }
            0
        }
        TRAY_MENU_DONE => {
            a.menu_open = false;
            match w {
                TRAY_CONFIG => a.configure(),
                TRAY_PAUSE => {
                    a.paused = !a.paused;
                    a.dismiss(true);
                }
                TRAY_QUIT => {
                    PostQuitMessage(0);
                    return 0;
                }
                _ => {}
            }
            a.refresh_registration();
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
                        a.input.is_some(),
                        a.input.is_some()
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
        _ => {
            // Default non-client handling can run Windows' modal move/size
            // loop. No application-state borrow may span that native loop:
            // its nested events must be able to enter our normal handlers.
            drop(_guard);
            DefWindowProcW(hwnd, msg, w, l)
        }
    }
}

// PeekMessage returns WM_QUIT even when it falls outside the requested filter.
// Foreground/cancel notifications queued by the tray menu may reach this drain
// before GetMessage sees Quit. Preserve it for the outer loop instead of eating it.
unsafe fn discard_gesture_packets(hwnd: HWND) {
    let mut stale: MSG = zeroed();
    while PeekMessageW(&mut stale, hwnd, INVOKE, CLICK, PM_REMOVE) != 0 {
        if stale.message == WM_QUIT {
            PostQuitMessage(stale.wParam as i32);
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confinement_check_uses_clip_bounds_not_motion_after_warp() {
        let clip = RECT {
            left: -1920,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        assert!(center_in_clip(POINT { x: 960, y: 540 }, clip));
        assert!(center_in_clip(POINT { x: -960, y: 540 }, clip));
        assert!(!center_in_clip(POINT { x: 1920, y: 540 }, clip));
        assert!(!center_in_clip(POINT { x: 0, y: 1080 }, clip));
        assert!(!center_in_clip(POINT { x: -1921, y: 0 }, clip));
    }

    // Each test owns a fresh thread queue. No windows, hooks, input or audio.
    #[test]
    fn reentrant_runtime_notifications_wait_for_outer_dispatch_and_own_no_os_pointer() {
        std::thread::spawn(|| unsafe {
            let mut message: MSG = zeroed();
            PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
            let outer = super::super::DispatchGuard::enter().unwrap();
            assert_eq!(defer_runtime_message(null_mut(), FOREGROUND, 0, 0), Some(0));
            assert_eq!(
                defer_runtime_message(null_mut(), WM_CAPTURECHANGED, 0, 0),
                Some(0)
            );
            assert_eq!(
                defer_runtime_message(null_mut(), config_reload::READY, 0, 0),
                Some(0)
            );
            assert_eq!(
                defer_runtime_message(null_mut(), WM_DPICHANGED, 0, 123),
                Some(0)
            );
            // These are the actual resident reentry routes that a native tray
            // menu used to retrieve and re-post while the outer borrow stayed held.
            assert_eq!(PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE), 0);
            drop(outer);
            let mut observed = vec![];
            while PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) != 0 {
                observed.push((message.message, message.lParam));
            }
            assert_eq!(
                observed,
                vec![
                    (FOREGROUND, 0),
                    (WM_CAPTURECHANGED, 0),
                    (config_reload::READY, 0),
                    (DEFERRED_DPI_CHANGE, 0)
                ]
            );
        })
        .join()
        .unwrap();
    }
    #[test]
    fn gesture_drain_preserves_quit_and_exit_code() {
        std::thread::spawn(|| unsafe {
            let mut message: MSG = zeroed();
            PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
            assert_ne!(PostThreadMessageW(GetCurrentThreadId(), INVOKE, 0, 0), 0);
            PostQuitMessage(37);
            discard_gesture_packets(null_mut());
            // Check before blocking so a regression fails instead of hanging.
            assert_ne!(PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE), 0);
            assert_eq!(message.message, WM_QUIT);
            assert_eq!(GetMessageW(&mut message, null_mut(), 0, 0), 0);
            assert_eq!(message.message, WM_QUIT);
            assert_eq!(message.wParam, 37);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn gesture_drain_removes_packets_but_keeps_other_notifications() {
        std::thread::spawn(|| unsafe {
            let mut message: MSG = zeroed();
            PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
            for kind in [
                INVOKE,
                RELEASE,
                KEY_ACTION,
                FOREGROUND,
                POINTER_MOVE,
                CLICK,
                RELOAD_CONFIG,
                config_reload::READY,
            ] {
                assert_ne!(PostThreadMessageW(GetCurrentThreadId(), kind, 0, 0), 0);
            }
            discard_gesture_packets(null_mut());
            assert_ne!(PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE), 0);
            assert_eq!(message.message, RELOAD_CONFIG);
            assert_ne!(PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE), 0);
            assert_eq!(message.message, config_reload::READY);
            assert_eq!(PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE), 0);
        })
        .join()
        .unwrap();
    }
}

#[cfg(test)]
mod native_modal_tests {
    use super::*;
    use std::cell::Cell;
    thread_local! { static PROBE: Cell<u32> = const { Cell::new(0) }; }
    unsafe extern "system" fn probe(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        match msg {
            WM_ENTERMENULOOP => {
                PROBE.with(|value| value.set(value.get() | 1));
                if let Some(guard) = super::super::DispatchGuard::enter() {
                    PROBE.with(|value| value.set(value.get() | 2));
                    drop(guard);
                }
                0
            }
            WM_TIMER => {
                EndMenu();
                0
            }
            _ => DefWindowProcW(hwnd, msg, w, l),
        }
    }
    #[test]
    #[ignore = "native overlay session; run on remote Windows CI only"]
    fn radial_opens_navigates_and_cancels_without_foreground_activation() {
        std::thread::spawn(|| unsafe {
            let module = GetModuleHandleW(null());
            let name = wide("Constellation.Test.NonactivatingRadial");
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: module,
                lpszClassName: name.as_ptr(),
                ..zeroed()
            };
            assert_ne!(RegisterClassW(&class), 0);
            let hwnd = CreateWindowExW(
                RADIAL_WINDOW_STYLE,
                name.as_ptr(),
                wide("test radial").as_ptr(),
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
            assert!(!hwnd.is_null());
            let root = std::env::temp_dir().join(format!(
                "constellation-overlay-{}-{}",
                std::process::id(),
                GetCurrentThreadId()
            ));
            let mut config = Config::default();
            config.sound.enabled = false;
            storage::save(&root, &config).unwrap();
            let bindings = config
                .profiles
                .iter()
                .map(|p| Binding::parse(&p.binding).unwrap())
                .collect();
            let foreground = GetForegroundWindow();
            let mut saved: POINT = zeroed();
            GetCursorPos(&mut saved);
            APP = Box::into_raw(Box::new(Runtime::new(
                hwnd,
                root,
                config,
                bindings,
                false,
                null_mut(),
            )));
            // Real production invocation and cancellation. This isolated fixture
            // intentionally has no Worker, so no global hooks are installed.
            SendMessageW(hwnd, INVOKE, 0, 0);
            assert_eq!((*APP).engine.profile, Some(0));
            assert_ne!(IsWindowVisible(hwnd), 0);
            assert_eq!(GetForegroundWindow(), foreground);
            assert!((*APP).selection_allowed());
            assert!((*APP).cursor.enabled);
            assert_eq!(
                SendMessageW(hwnd, WM_SETCURSOR, hwnd as usize, HTCLIENT as isize),
                1
            );
            let dot = GetCursor();
            let guard = super::super::DispatchGuard::enter().unwrap();
            assert_eq!(
                SendMessageW(hwnd, WM_SETCURSOR, hwnd as usize, HTCLIENT as isize),
                1
            );
            assert_eq!(
                GetCursor(),
                dot,
                "Reentrant cursor messages must not flash an arrow"
            );
            drop(guard);
            let mut icon: ICONINFO = zeroed();
            assert_ne!(GetIconInfo(dot, &mut icon), 0);
            assert_eq!(icon.fIcon, 0);
            assert!(icon.xHotspot > 0 && icon.xHotspot == icon.yHotspot);
            DeleteObject(icon.hbmMask);
            DeleteObject(icon.hbmColor);
            let mut rect: RECT = zeroed();
            GetWindowRect(hwnd, &mut rect);
            let blank = POINT {
                x: rect.left + 5,
                y: rect.top + 5,
            };
            assert_eq!(
                WindowFromPoint(blank),
                hwnd,
                "Blank radial areas must retain native cursor ownership"
            );
            let sector = (&(*APP).engine.sectors)[0].clone();
            let mut point = POINT {
                x: ((*APP).origin.x + sector.dot.x * (*APP).scale) as i32,
                y: ((*APP).origin.y + sector.dot.y * (*APP).scale) as i32,
            };
            ClientToScreen(hwnd, &mut point);
            assert_ne!(SetCursorPos(point.x, point.y), 0);
            // A stale pre-warp packet must not override the current position.
            SendMessageW(hwnd, POINTER_MOVE, 0, pack_point(blank));
            assert_eq!((*APP).engine.hovered, Some(sector.id));
            SendMessageW(hwnd, KEY_ACTION, 0x1b, 0);
            assert!((*APP).engine.profile.is_none());
            assert_eq!(IsWindowVisible(hwnd), 0);
            assert_eq!(GetForegroundWindow(), foreground);
            assert!(!(*APP).cursor.enabled);
            assert_ne!(GetCursor(), dot);
            // The no-selection Hold exit uses a separate close route.
            let mut config = (*APP).config.clone();
            config.profiles[0].mode = Mode::Hold;
            storage::save(&(*APP).root, &config).unwrap();
            SendMessageW(hwnd, INVOKE, 0, 0);
            assert!((*APP).cursor.enabled);
            SendMessageW(hwnd, RELEASE, 0, pack_point((*APP).center));
            assert!((*APP).engine.profile.is_none());
            assert!(!(*APP).cursor.enabled);
            assert_ne!(GetCursor(), dot);
            // A profile can keep the native system cursor instead.
            config.profiles[0].mode = Mode::Click;
            config.profiles[0].dot_cursor = false;
            storage::save(&(*APP).root, &config).unwrap();
            SendMessageW(hwnd, INVOKE, 0, 0);
            assert!(!(*APP).cursor.enabled);
            assert_ne!(WindowFromPoint(blank), hwnd);
            SendMessageW(hwnd, KEY_ACTION, 0x1b, 0);
            assert_eq!(GetForegroundWindow(), foreground);
            DestroyWindow(hwnd);
            drop(Box::from_raw(APP));
            APP = null_mut();
            SetCursorPos(saved.x, saved.y);
            UnregisterClassW(name.as_ptr(), module);
        })
        .join()
        .unwrap();
    }
    #[test]
    #[ignore = "native popup menu; run on remote Windows CI only"]
    fn tray_popup_keeps_dispatch_available_during_the_native_modal_loop() {
        std::thread::spawn(|| unsafe {
            let module = GetModuleHandleW(null());
            let class_name = wide("Constellation.Test.TrayModal");
            let class = WNDCLASSW {
                lpfnWndProc: Some(probe),
                hInstance: module,
                lpszClassName: class_name.as_ptr(),
                ..zeroed()
            };
            assert_ne!(RegisterClassW(&class), 0);
            let owner = CreateWindowExW(
                0,
                class_name.as_ptr(),
                wide("test").as_ptr(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                300,
                200,
                null_mut(),
                null_mut(),
                module,
                null(),
            );
            assert!(!owner.is_null());
            assert_ne!(SetTimer(owner, 1, 50, None), 0);
            // Uses production menu code. The timer ends the popup without
            // synthesizing any keyboard/mouse input or installing hooks.
            let guard = super::super::DispatchGuard::enter().unwrap();
            let command = track_tray_menu(owner, false, guard);
            KillTimer(owner, 1);
            DestroyWindow(owner);
            UnregisterClassW(class_name.as_ptr(), module);
            assert_eq!(command, 0);
            assert_eq!(
                PROBE.with(Cell::get),
                3,
                "native menu must run while application dispatch is available"
            );
        })
        .join()
        .unwrap();
    }
}
