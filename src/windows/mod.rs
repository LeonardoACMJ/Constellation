#![allow(unsafe_op_in_unsafe_fn)]
mod audio;
mod editor;
mod editor_theme;
mod input;
mod pickers;
mod recorder;
mod render;
mod runtime;
mod target;

use std::cell::Cell;
use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr::null_mut};
thread_local! { static DISPATCHING: Cell<bool> = const { Cell::new(false) }; }
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
        PostMessageW(hwnd, WM_CLOSE, w, l);
    }
    true
}

// Used only when the user explicitly invokes the launcher or opens an owned
// configuration window. Never retain an attachment to another input queue.
pub unsafe fn focus_window(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    use windows_sys::Win32::{
        System::Threading::{AttachThreadInput, GetCurrentThreadId},
        UI::Input::KeyboardAndMouse::SetFocus,
    };
    unsafe {
        SetForegroundWindow(hwnd);
        if GetForegroundWindow() != hwnd {
            let foreground_thread =
                GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
            let ours = GetCurrentThreadId();
            if foreground_thread != 0
                && foreground_thread != ours
                && AttachThreadInput(ours, foreground_thread, 1) != 0
            {
                SetForegroundWindow(hwnd);
                SetFocus(hwnd);
                AttachThreadInput(ours, foreground_thread, 0);
            }
        }
        GetForegroundWindow() == hwnd
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
    if args.iter().any(|a| a == "--configure") {
        editor::run(root)
    } else {
        runtime::run(root, args.iter().any(|a| a == "--smoke-test"))
    }
}
