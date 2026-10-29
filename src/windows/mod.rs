#![allow(unsafe_op_in_unsafe_fn)]
mod audio;
mod config_reload;
mod cursor;
mod editor;
mod editor_theme;
mod favicon;
mod icons;
mod input;
mod item_icons;
mod pickers;
mod recorder;
mod render;
mod runtime;
mod smooth;
mod startup;
mod target;

use std::cell::{Cell, RefCell};
use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr::null_mut};
#[derive(Clone, Copy)]
struct DeferredMessage {
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    w: usize,
    l: isize,
}
thread_local! {
    static DISPATCHING: Cell<bool> = const { Cell::new(false) };
    static DEFERRED: RefCell<Vec<DeferredMessage>> = const { RefCell::new(Vec::new()) };
}
/// Retain reentrant work until the owner releases its mutable state. Posting it
/// immediately lets a native modal loop retrieve and re-post it indefinitely.
pub(super) unsafe fn defer_message(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    w: usize,
    l: isize,
) {
    let message = DeferredMessage { hwnd, msg, w, l };
    if DISPATCHING.with(Cell::get) {
        DEFERRED.with(|pending| {
            let mut pending = pending.borrow_mut();
            // Only coalesce replaceable state notifications, never commands,
            // selection packets or messages owning heap/native payloads.
            if matches!(msg, WM_SIZE | runtime::POINTER_MOVE | runtime::FOREGROUND) {
                if let Some(last) = pending.last_mut() {
                    if last.hwnd == hwnd && last.msg == msg {
                        *last = message;
                        return;
                    }
                }
            }
            pending.push(message);
        });
    } else {
        unsafe {
            publish_deferred(message);
        }
    }
}
unsafe fn publish_deferred(message: DeferredMessage) {
    unsafe {
        if PostMessageW(message.hwnd, message.msg, message.w, message.l) == 0 {
            // The window can disappear while its owner is inside a dialog.
            if message.msg == input::ERROR && message.l != 0 {
                drop(Box::from_raw(message.l as *mut String));
            } else if message.msg == WM_DROPFILES && message.w != 0 {
                windows_sys::Win32::UI::Shell::DragFinish(message.w as _);
            }
        }
    }
}
// Win32 synchronously reenters window procedures during window/control changes.
// Prevent overlapping mutable borrows of the thread-owned native state.
pub(super) struct DispatchGuard;
impl DispatchGuard {
    pub fn enter() -> Option<Self> {
        DISPATCHING.with(|v| if v.replace(true) { None } else { Some(Self) })
    }
}
impl Drop for DispatchGuard {
    fn drop(&mut self) {
        DISPATCHING.with(|v| v.set(false));
        let pending = DEFERRED.with(|slot| std::mem::take(&mut *slot.borrow_mut()));
        for message in pending {
            unsafe {
                publish_deferred(message);
            }
        }
    }
}
use windows_sys::Win32::UI::WindowsAndMessaging::*;
// DefWindowProc(WM_SYSCOMMAND/SC_CLOSE) synchronously sends WM_CLOSE.
// A guarded nested dispatch must not pass that close back to DefWindowProc:
// it would destroy the window without running the owner's close/cleanup path.
pub(super) unsafe fn defer_nested_close(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    w: usize,
    l: isize,
) -> bool {
    if msg != WM_CLOSE {
        return false;
    }
    unsafe {
        defer_message(hwnd, WM_CLOSE, w, l);
    }
    true
}

// Request focus using normal Windows rules. Never attach input queues to
// another application: a blocked foreground thread must not couple our input.
pub unsafe fn focus_window(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    unsafe {
        SetForegroundWindow(hwnd);
        if GetForegroundWindow() != hwnd {
            return false;
        }
        windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus(hwnd);
        true
    }
}

pub fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}
pub fn error(s: &str) {
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(s).as_ptr(),
            wide("Constellation").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
pub fn run() -> Result<(), String> {
    use windows_sys::Win32::UI::HiDpi::*;
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let root = crate::storage::root().map_err(|e| e.to_string())?;
    let args: Vec<_> = std::env::args().collect();
    if args.iter().any(|a| a == "--check-config") {
        let c = if root.join("data/config.json").exists() {
            crate::storage::read(&root)?
        } else {
            crate::storage::load_or_create(&root)?
        };
        c.validate()?;
        std::fs::write(
            root.join("data/config-check.txt"),
            "Configuration is valid.\r\n",
        )
        .map_err(|e| e.to_string())?;
        return Ok(());
    }
    // A Start menu launch opens settings when the resident is already available.
    // Otherwise it starts the tray app just like launching the portable executable.
    let menu_with_resident = args.iter().any(|a| a == "--start-menu")
        && unsafe {
            !FindWindowW(
                wide("Constellation.Native.Radial").as_ptr(),
                std::ptr::null(),
            )
            .is_null()
        };
    if args.iter().any(|a| a == "--configure") || menu_with_resident {
        editor::run(root)
    } else {
        runtime::run(root, args.iter().any(|a| a == "--smoke-test"))
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;
    use std::mem::zeroed;
    #[test]
    fn modal_reentry_does_not_feed_its_own_message_queue() {
        std::thread::spawn(|| unsafe {
            let mut message = zeroed();
            PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
            let outer = DispatchGuard::enter().unwrap();
            assert!(DispatchGuard::enter().is_none());
            defer_message(null_mut(), WM_APP + 100, 7, 9);
            // A nested native loop must have nothing to retrieve/re-defer.
            for _ in 0..100 {
                assert_eq!(
                    PeekMessageW(
                        &mut message,
                        null_mut(),
                        WM_APP + 100,
                        WM_APP + 100,
                        PM_REMOVE
                    ),
                    0
                );
            }
            drop(outer);
            assert_ne!(
                PeekMessageW(
                    &mut message,
                    null_mut(),
                    WM_APP + 100,
                    WM_APP + 100,
                    PM_REMOVE
                ),
                0
            );
            assert_eq!((message.wParam, message.lParam), (7, 9));
            assert_eq!(
                PeekMessageW(
                    &mut message,
                    null_mut(),
                    WM_APP + 100,
                    WM_APP + 100,
                    PM_REMOVE
                ),
                0
            );
        })
        .join()
        .unwrap();
    }
    #[test]
    fn deferred_state_bursts_are_coalesced_without_losing_commands() {
        std::thread::spawn(|| unsafe {
            let mut message = zeroed();
            PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
            let outer = DispatchGuard::enter().unwrap();
            for value in 0..1000 {
                defer_message(null_mut(), WM_SIZE, 0, value);
            }
            defer_message(null_mut(), WM_COMMAND, 401, 1);
            defer_message(null_mut(), WM_COMMAND, 401, 2);
            assert_eq!(DEFERRED.with(|pending| pending.borrow().len()), 3);
            drop(outer);
            let mut observed = vec![];
            while PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) != 0 {
                observed.push((message.message, message.wParam, message.lParam));
            }
            assert_eq!(
                observed,
                vec![
                    (WM_SIZE, 0, 999),
                    (WM_COMMAND, 401, 1),
                    (WM_COMMAND, 401, 2)
                ]
            );
        })
        .join()
        .unwrap();
    }
}
