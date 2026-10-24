#![allow(unsafe_op_in_unsafe_fn)]
//! Input owns a dedicated message thread. No layout, launch, file I/O or audio
//! work occurs on that thread. UI synchronization holds the mutex only for data.
use super::runtime::{
    CLICK, FOREGROUND, INVOKE, KEY_ACTION, POINTER_MOVE, RELEASE, foreground_blocks_launcher,
    pack_point, pointer_packet,
};
use crate::config::{Binding, Input, Mode};
use std::{
    cell::{Cell, RefCell},
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
const OBSERVED: u32 = WM_APP + 44;
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
    observed_foreground: HWND,
    observed_blocked: bool,
}
// HWNDs here are opaque cross-thread notification/comparison handles. Their
// owner creates and destroys the window only after this worker has stopped.
unsafe impl Send for HookState {}
type Shared = Arc<Mutex<HookState>>;
thread_local! {
    static CONTEXT: RefCell<Option<Shared>> = const { RefCell::new(None) };
    static INPUT_THREAD: Cell<u32> = const { Cell::new(0) };
}
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
            observed_foreground: null_mut(),
            observed_blocked: true,
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
                // Start observation before installing synchronous input hooks.
                let observer = Observer::start(state.clone(), GetCurrentThreadId());
                let module = GetModuleHandleW(null());
                let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module, 0);
                let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0);
                let ok = !keyboard.is_null() && !mouse.is_null() && observer.is_ok();
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
                            OBSERVED => refresh(&state, false),
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
                // Remove global hooks before waiting for observer shutdown.
                drop(observer);
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
struct Observer {
    id: u32,
    thread: Option<JoinHandle<()>>,
}
impl Observer {
    unsafe fn start(state: Shared, input_thread: u32) -> Result<Self, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("constellation-window-observer".into())
            .stack_size(512 * 1024)
            .spawn(move || unsafe {
                let mut message: MSG = zeroed();
                PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
                CONTEXT.with(|slot| *slot.borrow_mut() = Some(state.clone()));
                INPUT_THREAD.with(|slot| slot.set(input_thread));
                let foreground = SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    null_mut(),
                    Some(foreground_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                );
                let location = SetWinEventHook(
                    EVENT_OBJECT_LOCATIONCHANGE,
                    EVENT_OBJECT_LOCATIONCHANGE,
                    null_mut(),
                    Some(foreground_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                );
                let ok = !foreground.is_null() && !location.is_null();
                // Publish readiness before inspection; startup must not wait on
                // another application's window while global hooks are installed.
                let _ = tx.send(if ok {
                    Ok(GetCurrentThreadId())
                } else {
                    Err("Cannot observe foreground windows".to_string())
                });
                if ok {
                    observe(&state, input_thread);
                    while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                        if message.message == STOP {
                            break;
                        }
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
                if !foreground.is_null() {
                    UnhookWinEvent(foreground);
                }
                if !location.is_null() {
                    UnhookWinEvent(location);
                }
                CONTEXT.with(|slot| slot.borrow_mut().take());
            })
            .map_err(|e| e.to_string())?;
        match rx.recv().map_err(|e| e.to_string())? {
            Ok(id) => Ok(Self {
                id,
                thread: Some(thread),
            }),
            Err(error) => {
                let _ = thread.join();
                Err(error)
            }
        }
    }
}
impl Drop for Observer {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(self.id, STOP, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
unsafe fn observe(state: &Shared, input_thread: u32) {
    let destination = state.lock().unwrap().hwnd;
    let foreground = GetForegroundWindow();
    let blocked = foreground_blocks_launcher(destination);
    // If focus changed during inspection, wait for its own observation rather
    // than applying another window's fullscreen status to the new foreground.
    let changed_during_inspection = GetForegroundWindow() != foreground;
    {
        let mut h = state.lock().unwrap();
        h.observed_foreground = foreground;
        h.observed_blocked = blocked || changed_during_inspection;
    }
    PostThreadMessageW(input_thread, OBSERVED, 0, 0);
    PostMessageW(destination, FOREGROUND, 0, 0);
}
fn hook_allowed(h: &HookState, foreground: HWND) -> bool {
    // The owned radial can receive its first click before its asynchronous
    // foreground observation arrives. Its active session is already authorized.
    if !foreground.is_null() && foreground == h.hwnd && h.active.is_some() {
        return h.available;
    }
    h.enabled
        && h.foreground == foreground
        && h.observed_foreground == foreground
        && !h.observed_blocked
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
    let foreground = GetForegroundWindow();
    let mut h = state.lock().unwrap();
    let hwnd = h.hwnd;
    let enabled = h.available && h.observed_foreground == foreground && !h.observed_blocked;
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
    observe(&state, INPUT_THREAD.with(Cell::get));
}
unsafe extern "system" fn keyboard_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(null_mut(), code, w, l);
    }
    let Some(state) = context() else {
        return CallNextHookEx(null_mut(), code, w, l);
    };
    let foreground = GetForegroundWindow();
    let consumed = {
        // Never wait for a UI/configuration lock while Windows waits for input.
        let Ok(mut h) = state.try_lock() else {
            return CallNextHookEx(null_mut(), code, w, l);
        };
        let blocked = !hook_allowed(&h, foreground);
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
    let foreground = GetForegroundWindow();
    let consumed = {
        let Ok(mut h) = state.try_lock() else {
            return CallNextHookEx(null_mut(), code, w, l);
        };
        let blocked = !hook_allowed(&h, foreground);
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
        if let Some(i) = active.filter(|_| !blocked) {
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
    if blocked {
        return false;
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
        if let Some(i) = active.filter(|_| !blocked) {
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

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> HookState {
        HookState {
            hwnd: 1usize as HWND,
            bindings: vec![Binding::parse("Ctrl+Q").unwrap()],
            modes: vec![Mode::Click],
            registered: vec![true],
            keys: [false; 256],
            eaten: [false; 256],
            mouse_eaten: [false; 6],
            active: None,
            available: true,
            enabled: true,
            reserved: 0,
            failures: vec![],
            foreground: 2usize as HWND,
            observed_foreground: 2usize as HWND,
            observed_blocked: false,
        }
    }
    #[test]
    fn stale_or_fullscreen_observation_never_authorizes_a_shortcut() {
        let mut h = state();
        assert!(hook_allowed(&h, 2usize as HWND));
        assert!(!hook_allowed(&h, 3usize as HWND));
        h.observed_blocked = true;
        assert!(!hook_allowed(&h, 2usize as HWND));
        h.keys[0x11] = true;
        let key = KBDLLHOOKSTRUCT {
            vkCode: 0x51,
            ..unsafe { zeroed() }
        };
        assert!(!unsafe { keyboard_packet(&mut h, WM_KEYDOWN as usize, &key, true) });
        assert!(!h.eaten[0x51]);
        assert!(h.active.is_none());
    }
    #[test]
    fn owned_radial_accepts_immediate_selection_without_an_observer_round_trip() {
        let mut h = state();
        h.active = Some(0);
        h.observed_blocked = true;
        assert!(hook_allowed(&h, h.hwnd));
        h.available = false;
        assert!(!hook_allowed(&h, h.hwnd));
    }
    #[test]
    fn ordinary_desktop_clicks_pass_through_without_a_radial_session() {
        let mut h = state();
        let event: MSLLHOOKSTRUCT = unsafe { zeroed() };
        for blocked in [false, true] {
            for message in [WM_LBUTTONDOWN, WM_LBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONUP] {
                assert!(!unsafe { mouse_packet(&mut h, message as usize, &event, blocked) });
            }
        }
        assert!(h.mouse_eaten.iter().all(|v| !v));
    }
    #[test]
    fn finishing_an_eaten_click_clears_it_even_after_fullscreen_transition() {
        let mut h = state();
        h.mouse_eaten[1] = true;
        let event: MSLLHOOKSTRUCT = unsafe { zeroed() };
        assert!(unsafe { mouse_packet(&mut h, WM_LBUTTONUP as usize, &event, true) });
        assert!(!unsafe { mouse_packet(&mut h, WM_LBUTTONDOWN as usize, &event, true) });
        assert!(!h.mouse_eaten[1]);
    }
}
