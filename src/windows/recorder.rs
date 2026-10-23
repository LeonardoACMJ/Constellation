//! Short-lived raw mouse recording, using the same button identities as the
//! resident hooks. No editor-state borrow crosses a native hook callback.
use std::ptr::null_mut;
use windows_sys::Win32::{
    Foundation::*, System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*,
};
pub const CAPTURE: u32 = WM_APP + 30;
static mut DESTINATION: HWND = null_mut();
static mut HOOK: HHOOK = null_mut();
static mut CONSUMED: u16 = 0;
pub unsafe fn start(hwnd: HWND) -> Result<(), String> {
    unsafe {
        if !HOOK.is_null() {
            UnhookWindowsHookEx(HOOK);
            HOOK = null_mut();
        }
        CONSUMED = 0;
        DESTINATION = hwnd;
        HOOK = SetWindowsHookExW(
            WH_MOUSE_LL,
            Some(mouse),
            GetModuleHandleW(std::ptr::null()),
            0,
        );
        if HOOK.is_null() {
            DESTINATION = null_mut();
            return Err("Mouse recorder unavailable; type the binding instead.".into());
        }
        Ok(())
    }
}
pub unsafe fn stop() {
    unsafe {
        DESTINATION = null_mut();
        // Keep the hook just long enough to eat the corresponding button-up.
        if CONSUMED == 0 && !HOOK.is_null() {
            UnhookWindowsHookEx(HOOK);
            HOOK = null_mut();
        }
    }
}
unsafe extern "system" fn mouse(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if code < 0 {
            return CallNextHookEx(null_mut(), code, w, l);
        }
        let event = &*(l as *const MSLLHOOKSTRUCT);
        let (button, down) = match w as u32 {
            WM_MBUTTONDOWN => (3, true),
            WM_MBUTTONUP => (3, false),
            WM_XBUTTONDOWN | WM_XBUTTONUP => (
                if event.mouseData >> 16 == 1 { 4 } else { 5 },
                w as u32 == WM_XBUTTONDOWN,
            ),
            _ => return CallNextHookEx(null_mut(), code, w, l),
        };
        if !down && CONSUMED == button {
            CONSUMED = 0;
            if DESTINATION.is_null() && !HOOK.is_null() {
                UnhookWindowsHookEx(HOOK);
                HOOK = null_mut();
            }
            return 1;
        }
        if down && !DESTINATION.is_null() {
            CONSUMED = button;
            PostMessageW(DESTINATION, CAPTURE, button as usize, 0);
            return 1;
        }
        CallNextHookEx(null_mut(), code, w, l)
    }
}
