//! Native clipping/scroll containers. No editor-state borrow crosses forwarding.
#![allow(unsafe_op_in_unsafe_fn)]
use super::{editor_theme as theme, render, wide};
use std::{mem::zeroed, ptr::null};
use windows_sys::Win32::{
    Foundation::*, System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*,
};
pub const SCROLL: u32 = WM_APP + 80;
pub const WHEEL: u32 = WM_APP + 81;
pub const REVEAL: u32 = WM_APP + 82;
pub unsafe fn create(owner: HWND, id: i32) -> HWND {
    let module = GetModuleHandleW(null());
    let class = wide("Constellation.EditorPane");
    RegisterClassW(&WNDCLASSW {
        lpfnWndProc: Some(proc),
        hInstance: module,
        lpszClassName: class.as_ptr(),
        ..zeroed()
    });
    CreateWindowExW(
        WS_EX_CONTROLPARENT,
        class.as_ptr(),
        wide("").as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
        0,
        0,
        1,
        1,
        owner,
        id as usize as HMENU,
        module,
        null(),
    )
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(result) = theme::message(msg, w, l) {
        return result;
    }
    match msg {
        WM_PAINT => {
            render::buffered(hwnd, theme::background(), |_| {});
            0
        }
        WM_COMMAND | WM_NOTIFY | WM_HSCROLL => SendMessageW(GetParent(hwnd), msg, w, l),
        WM_VSCROLL => {
            PostMessageW(
                GetParent(hwnd),
                SCROLL,
                GetDlgCtrlID(hwnd) as usize,
                w as isize,
            );
            0
        }
        WM_MOUSEWHEEL => {
            PostMessageW(
                GetParent(hwnd),
                WHEEL,
                GetDlgCtrlID(hwnd) as usize,
                w as isize,
            );
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}
