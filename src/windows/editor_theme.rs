//! Editor-only native resources. Nothing is allocated in the tray process.
#![allow(unsafe_op_in_unsafe_fn)]
use super::{render, wide};
use std::cell::RefCell;
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    UI::{Controls::*, WindowsAndMessaging::*},
};
pub const BACKGROUND: u32 = render::rgb(15, 22, 36);
pub const TEXT: u32 = render::rgb(233, 237, 250);
pub const MUTED: u32 = render::rgb(148, 164, 190);
const INPUT: u32 = render::rgb(23, 33, 50);
struct Theme {
    background: HBRUSH,
    input: HBRUSH,
    font: HFONT,
}
impl Drop for Theme {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.background);
            DeleteObject(self.input);
            DeleteObject(self.font);
        }
    }
}
thread_local! {static THEME:RefCell<Option<Theme>>=const {RefCell::new(None)};}
pub unsafe fn install() {
    let font = CreateFontW(
        -16,
        0,
        0,
        0,
        400,
        0,
        0,
        0,
        DEFAULT_CHARSET as u32,
        OUT_DEFAULT_PRECIS as u32,
        CLIP_DEFAULT_PRECIS as u32,
        CLEARTYPE_QUALITY as u32,
        DEFAULT_PITCH as u32,
        wide("Segoe UI").as_ptr(),
    );
    THEME.with(|v| {
        *v.borrow_mut() = Some(Theme {
            background: CreateSolidBrush(BACKGROUND),
            input: CreateSolidBrush(INPUT),
            font,
        })
    });
}
pub fn shutdown() {
    THEME.with(|v| v.borrow_mut().take());
}
pub unsafe fn window(hwnd: HWND) {
    let dark = 1i32;
    DwmSetWindowAttribute(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
        &dark as *const _ as _,
        4,
    );
}
pub unsafe fn control(hwnd: HWND, class: &str) {
    THEME.with(|v| {
        if let Some(t) = v.borrow().as_ref() {
            SendMessageW(hwnd, WM_SETFONT, t.font as usize, 0);
        }
    });
    // Use documented classic control painting so our palette is respected.
    if matches!(
        class,
        "BUTTON" | "EDIT" | "LISTBOX" | "COMBOBOX" | "msctls_trackbar32"
    ) {
        SetWindowTheme(hwnd, wide("").as_ptr(), wide("").as_ptr());
    }
}
/// Runs before the editor's mutable dispatch guard. Native control painting can
/// reenter its parent while an edit/layout operation owns the editor state.
pub unsafe fn message(msg: u32, w: WPARAM, l: LPARAM) -> Option<LRESULT> {
    if msg == WM_ERASEBKGND {
        return Some(1);
    }
    if matches!(
        msg,
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX | WM_CTLCOLORDLG
    ) {
        return THEME.with(|v| {
            v.borrow().as_ref().map(|t| {
                let dc = w as HDC;
                let input = matches!(msg, WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX);
                SetTextColor(dc, TEXT);
                SetBkColor(dc, if input { INPUT } else { BACKGROUND });
                if !input {
                    SetBkMode(dc, TRANSPARENT as i32);
                }
                (if input { t.input } else { t.background }) as LRESULT
            })
        });
    }
    if msg == WM_DRAWITEM && l != 0 {
        let item = &*(l as *const DRAWITEMSTRUCT);
        if item.CtlType == ODT_BUTTON {
            draw_button(item);
            return Some(1);
        }
    }
    None
}
unsafe fn draw_button(item: &DRAWITEMSTRUCT) {
    let dc = item.hDC;
    let r = item.rcItem;
    let disabled = item.itemState & ODS_DISABLED != 0;
    let down = item.itemState & ODS_SELECTED != 0;
    let primary = matches!(item.CtlID, 401 | 411 | 610);
    let color = if down {
        render::rgb(55, 98, 105)
    } else if primary {
        render::rgb(40, 100, 102)
    } else {
        render::rgb(30, 43, 62)
    };
    render::fill(dc, &r, BACKGROUND);
    let brush = CreateSolidBrush(color);
    let pen = CreatePen(
        PS_SOLID,
        1,
        if primary {
            render::rgb(115, 191, 182)
        } else {
            render::rgb(77, 89, 117)
        },
    );
    let oldb = SelectObject(dc, brush);
    let oldp = SelectObject(dc, pen);
    RoundRect(dc, r.left, r.top, r.right, r.bottom, 8, 8);
    SelectObject(dc, oldb);
    SelectObject(dc, oldp);
    DeleteObject(brush);
    DeleteObject(pen);
    let len = GetWindowTextLengthW(item.hwndItem);
    let mut text = vec![0u16; len as usize + 1];
    GetWindowTextW(item.hwndItem, text.as_mut_ptr(), text.len() as i32);
    let old = THEME.with(|v| v.borrow().as_ref().map(|t| SelectObject(dc, t.font)));
    SetTextColor(dc, if disabled { MUTED } else { TEXT });
    SetBkMode(dc, TRANSPARENT as i32);
    let mut text_rect = r;
    DrawTextW(
        dc,
        text.as_ptr(),
        len,
        &mut text_rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
    if item.itemState & ODS_FOCUS != 0 {
        let mut focus = r;
        InflateRect(&mut focus, -4, -4);
        DrawFocusRect(dc, &focus);
    }
    if let Some(old) = old {
        SelectObject(dc, old);
    }
}
pub unsafe fn panel(dc: HDC, r: RECT) {
    let pen = CreatePen(PS_SOLID, 1, render::rgb(43, 57, 80));
    let old = SelectObject(dc, pen);
    let brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
    RoundRect(dc, r.left, r.top, r.right, r.bottom, 10, 10);
    SelectObject(dc, old);
    SelectObject(dc, brush);
    DeleteObject(pen);
}
