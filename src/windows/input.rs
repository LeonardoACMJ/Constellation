#![allow(unsafe_op_in_unsafe_fn)]
//! Input owns a dedicated message thread. No layout, launch, file I/O or audio
//! work occurs on that thread. UI synchronization holds the mutex only for data.
use super::runtime::{
    CLICK, FOREGROUND, INVOKE, KEY_ACTION, POINTER_MOVE, RELEASE, foreground_blocks_launcher,
    pack_point, pointer_packet,
};
use crate::config::{Binding, Input, Mode};
use std::{
    cell::RefCell,
    mem::zeroed,
    ptr::{null, null_mut},
    sync::{Arc, Mutex},
    thread::JoinHandle,
};
use windows_sys::Win32::{
    Foundation::*,
    System::{LibraryLoader::GetModuleHandleW, Threading::*},
    UI::{Accessibility::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
const UPDATE: u32 = WM_APP + 40;
const RESET: u32 = WM_APP + 41;
const STOP: u32 = WM_APP + 42;
pub const ERROR: u32 = WM_APP + 43;
struct HookState {
    hwnd: HWND,
    bindings: Vec<Binding>,
    modes: Vec<Mode>,
    registered: Vec<bool>,
    keys: [bool; 256],
    eaten: [bool; 256],
    mouse_eaten: [bool; 6],
    active: Option<usize>,
    available: bool,
    enabled: bool,
    reserved: usize,
    failures: Vec<String>,
    foreground: HWND,
}
// HWNDs here are opaque cross-thread notification/comparison handles. Their
// owner creates and destroys the window only after this worker has stopped.
unsafe impl Send for HookState {}
type Shared = Arc<Mutex<HookState>>;
thread_local! {static CONTEXT:RefCell<Option<Shared>>=const {RefCell::new(None)};}
fn context() -> Option<Shared> {
    CONTEXT.with(|v| v.borrow().clone())
}
pub struct Worker {
    shared: Shared,
    id: u32,
    thread: Option<JoinHandle<()>>,
}
impl Worker {
    pub unsafe fn start(
        hwnd: HWND,
        bindings: Vec<Binding>,
        modes: Vec<Mode>,
    ) -> Result<Self, String> {
        let shared = Arc::new(Mutex::new(HookState {
            hwnd,
            bindings,
            modes,
            registered: vec![],
            keys: [false; 256],
            eaten: [false; 256],
            mouse_eaten: [false; 6],
            active: None,
            available: false,
            enabled: false,
            reserved: 0,
            failures: vec![],
            foreground: null_mut(),
        }));
        let state = shared.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("constellation-input".into())
            .stack_size(512 * 1024)
            .spawn(move || unsafe {
                let mut msg: MSG = zeroed();
                PeekMessageW(&mut msg, null_mut(), 0, 0, PM_NOREMOVE);
                CONTEXT.with(|v| *v.borrow_mut() = Some(state.clone()));
                reset(&state);
                let module = GetModuleHandleW(null());
                let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module, 0);
                let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0);
                let foreground = SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    null_mut(),
                    Some(foreground_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
                let location = SetWinEventHook(
                    EVENT_OBJECT_LOCATIONCHANGE,
                    EVENT_OBJECT_LOCATIONCHANGE,
                    null_mut(),
                    Some(foreground_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
                let ok = !keyboard.is_null()
                    && !mouse.is_null()
                    && !foreground.is_null()
                    && !location.is_null();
                let _ = tx.send(if ok {
                    Ok(GetCurrentThreadId())
                } else {
                    Err("Windows input hooks could not be installed".to_string())
                });
                if ok {
                    while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                        match msg.message {
                            STOP => break,
                            UPDATE => refresh(&state, true),
                            RESET => reset(&state),
                            _ => {
                                TranslateMessage(&msg);
                                DispatchMessageW(&msg);
                            }
                        }
                    }
                }
                let reserved = state.lock().unwrap().reserved;
                for i in 0..reserved {
                    UnregisterHotKey(null_mut(), i as i32 + 100);
                }
                if !keyboard.is_null() {
                    UnhookWindowsHookEx(keyboard);
                }
                if !mouse.is_null() {
                    UnhookWindowsHookEx(mouse);
                }
                if !foreground.is_null() {
                    UnhookWinEvent(foreground);
                }
                if !location.is_null() {
                    UnhookWinEvent(location);
                }
                CONTEXT.with(|v| *v.borrow_mut() = None);
            })
            .map_err(|e| e.to_string())?;
        match rx.recv().map_err(|e| e.to_string())? {
            Ok(id) => Ok(Self {
                shared,
                id,
                thread: Some(thread),
            }),
            Err(e) => {
                let _ = thread.join();
                Err(e)
            }
        }
    }
    pub unsafe fn configure(&self, bindings: Vec<Binding>, modes: Vec<Mode>, available: bool) {
        {
            let mut h = self.shared.lock().unwrap();
            h.enabled = false;
            h.bindings = bindings;
            h.modes = modes;
            h.available = available;
        }
        PostThreadMessageW(self.id, UPDATE, 0, 0);
    }
    pub fn active(&self, value: Option<usize>) {
        self.shared.lock().unwrap().active = value;
    }
    pub unsafe fn resync(&self) {
        PostThreadMessageW(self.id, RESET, 0, 0);
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(self.id, STOP, 0, 0);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
unsafe fn reset(state: &Shared) {
    let mut keys = [false; 256];
    for (i, k) in keys.iter_mut().enumerate() {
        *k = GetAsyncKeyState(i as i32) < 0;
    }
    let mut h = state.lock().unwrap();
    h.keys = keys;
    for (i, k) in keys.iter().enumerate() {
        h.eaten[i] &= *k;
    }
    for (button, vk) in [(1, 1), (2, 2), (3, 4), (4, 5), (5, 6)] {
        h.mouse_eaten[button] &= keys[vk];
    }
}
unsafe fn refresh(state: &Shared, force: bool) {
    let hwnd = state.lock().unwrap().hwnd;
    let foreground = GetForegroundWindow();
    let blocked = foreground_blocks_launcher(hwnd);
    let mut h = state.lock().unwrap();
    let enabled = h.available && !blocked;
    if !force && enabled == h.enabled && foreground == h.foreground {
        return;
    }
    for i in 0..h.reserved {
        UnregisterHotKey(null_mut(), i as i32 + 100);
    }
    h.reserved = h.bindings.len();
    h.registered = vec![false; h.bindings.len()];
    h.enabled = enabled;
    h.foreground = foreground;
    let mut failures = vec![];
    if enabled {
        for i in 0..h.bindings.len() {
            let b = &h.bindings[i];
            let ok = if let Input::Key(vk) = b.key {
                let mods = (if b.ctrl { MOD_CONTROL } else { 0 })
                    | (if b.shift { MOD_SHIFT } else { 0 })
                    | (if b.alt { MOD_ALT } else { 0 })
                    | MOD_NOREPEAT;
                RegisterHotKey(null_mut(), i as i32 + 100, mods, vk as u32) != 0
            } else {
                true
            };
            h.registered[i] = ok;
            if !ok {
                failures.push(format!("{:?}", h.bindings[i]));
            }
        }
    }
    let changed = failures != h.failures;
    h.failures = failures.clone();
    drop(h);
    if changed && !failures.is_empty() {
        let message = Box::new(format!(
            "Unavailable global bindings: {}. Other profiles remain usable.",
            failures.join(", ")
        ));
        let ptr = Box::into_raw(message);
        if PostMessageW(hwnd, ERROR, 0, ptr as LPARAM) == 0 {
            drop(Box::from_raw(ptr));
        }
    }
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
    let Some(state) = context() else {
        return;
    };
    if event == EVENT_OBJECT_LOCATIONCHANGE && (hwnd != GetForegroundWindow() || object != 0) {
        return;
    }
    refresh(&state, false);
    let destination = state.lock().unwrap().hwnd;
    PostMessageW(destination, FOREGROUND, 0, 0);
}
unsafe extern "system" fn keyboard_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(null_mut(), code, w, l);
    }
    let Some(state) = context() else {
        return CallNextHookEx(null_mut(), code, w, l);
    };
    refresh(&state, false);
    let consumed = {
        let mut h = state.lock().unwrap();
        let blocked = !h.enabled;
        keyboard_packet(&mut h, w, &*(l as *const KBDLLHOOKSTRUCT), blocked)
    };
    if consumed {
        1
    } else {
        CallNextHookEx(null_mut(), code, w, l)
    }
}
unsafe extern "system" fn mouse_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(null_mut(), code, w, l);
    }
    let Some(state) = context() else {
        return CallNextHookEx(null_mut(), code, w, l);
    };
    if w as u32 != WM_MOUSEMOVE {
        refresh(&state, false);
    }
    let consumed = {
        let mut h = state.lock().unwrap();
        let blocked = !h.enabled;
        mouse_packet(&mut h, w, &*(l as *const MSLLHOOKSTRUCT), blocked)
    };
    if consumed {
        1
    } else {
        CallNextHookEx(null_mut(), code, w, l)
    }
}
unsafe fn keyboard_packet(
    a: &mut HookState,
    w: WPARAM,
    e: &KBDLLHOOKSTRUCT,
    blocked: bool,
) -> bool {
    let vk = e.vkCode as usize;
    if vk >= 256 {
        return false;
    }
    let down = matches!(w as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
    let up = matches!(w as u32, WM_KEYUP | WM_SYSKEYUP);
    if !down && !up {
        return false;
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
    let active = a.active;
    if up {
        if let Some(i) = active {
            if a.modes.get(i) == Some(&Mode::Hold) && a.bindings[i].requires(Input::Key(vk as u16))
            {
                PostMessageW(a.hwnd, RELEASE, i, pointer_packet());
            }
        }
        let consumed = a.eaten[vk];
        a.eaten[vk] = false;
        return consumed;
    }
    if a.eaten[vk] {
        return true;
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
        if !a.enabled || blocked {
            PostMessageW(a.hwnd, FOREGROUND, 0, 0);
            return false;
        }
        a.eaten[vk] = true;
        if !repeat {
            if active.is_none() {
                a.active = Some(i);
                PostMessageW(a.hwnd, INVOKE, i, 0);
            } else if active == Some(i) && a.modes[i] == Mode::Click {
                PostMessageW(a.hwnd, INVOKE, i, 0);
            }
        }
        return true;
    }
    if active.is_some() && a.enabled && !blocked && !repeat && matches!(vk, 0x1b | 0x20 | 0x08) {
        a.eaten[vk] = true;
        PostMessageW(a.hwnd, KEY_ACTION, vk, pointer_packet());
        return true;
    }
    false
}

unsafe fn mouse_packet(a: &mut HookState, w: WPARAM, e: &MSLLHOOKSTRUCT, blocked: bool) -> bool {
    let msg = w as u32;
    if msg == WM_MOUSEMOVE {
        if a.active.is_some() {
            PostMessageW(a.hwnd, POINTER_MOVE, 0, pack_point(e.pt));
        }
        return false;
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
        _ => return false,
    };
    let active = a.active;
    if !down {
        if let Some(i) = active {
            if a.modes.get(i) == Some(&Mode::Hold)
                && a.bindings
                    .get(i)
                    .is_some_and(|b| b.key == Input::Mouse(button))
            {
                PostMessageW(a.hwnd, RELEASE, i, pack_point(e.pt));
            }
        }
        let consumed = a.mouse_eaten[button as usize];
        a.mouse_eaten[button as usize] = false;
        return consumed;
    }
    if !a.enabled || blocked {
        if active.is_some() {
            PostMessageW(a.hwnd, FOREGROUND, 0, 0);
        }
        return false;
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
            a.active = Some(i);
            PostMessageW(a.hwnd, INVOKE, i, 0);
        } else if active == Some(i) && a.modes[i] == Mode::Click {
            PostMessageW(a.hwnd, INVOKE, i, 0);
        }
        return true;
    }
    if active.is_some() && matches!(button, 1 | 2) {
        a.mouse_eaten[button as usize] = true;
        PostMessageW(a.hwnd, CLICK, button as usize, pack_point(e.pt));
        return true;
    }
    false
}
