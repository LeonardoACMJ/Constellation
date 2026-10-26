//! Editor-only native resources. Nothing is allocated in the tray process.
#![allow(unsafe_op_in_unsafe_fn)]
use super::{render, wide};
use std::cell::RefCell;
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    UI::{
        Controls::*,
        Input::KeyboardAndMouse::{GetFocus, IsWindowEnabled},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
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
    if class == "EDIT" {
        SetWindowSubclass(hwnd, Some(edit_proc), 2, 0);
    }
    if class == "SysTreeView32" {
        SetWindowTheme(hwnd, wide("").as_ptr(), wide("").as_ptr());
        SendMessageW(hwnd, TVM_SETBKCOLOR, 0, INPUT as isize);
        SendMessageW(hwnd, TVM_SETTEXTCOLOR, 0, TEXT as isize);
        SendMessageW(hwnd, TVM_SETLINECOLOR, 0, render::rgb(65, 81, 105) as isize);
        SendMessageW(hwnd, TVM_SETINDENT, 20, 0);
        SendMessageW(hwnd, TVM_SETITEMHEIGHT, 24, 0);
    }
    if class == "msctls_trackbar32" {
        // Preserve native input/accessibility; replace only the paint path.
        SetWindowSubclass(hwnd, Some(slider_proc), 1, 0);
    }
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

/// Classic EDIT does not provide the expected Ctrl+A / Ctrl+Backspace
/// behaviour. Handle their translated characters without global key hooks,
/// clipboard access, or replacing the control's native undo machinery.
unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_CHAR if w == 1 => {
            SendMessageW(hwnd, EM_SETSEL, 0, -1);
            0
        }
        WM_CHAR if w == 0x7f => {
            if GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & ES_READONLY as u32 != 0
                || IsWindowEnabled(hwnd) == 0
            {
                return 0;
            }
            let length = GetWindowTextLengthW(hwnd).max(0) as usize;
            let mut text = vec![0u16; length + 1];
            let read = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32).max(0) as usize;
            text.truncate(read);
            let (mut start, mut end) = (0u32, 0u32);
            SendMessageW(
                hwnd,
                EM_GETSEL,
                &mut start as *mut _ as usize,
                &mut end as *mut _ as isize,
            );
            let mut start = (start as usize).min(read);
            let mut end = (end as usize).min(read);
            // Also tolerate a programmatically placed caret inside a pair.
            let inside_pair = |p: usize| {
                p > 0
                    && p < text.len()
                    && (0xd800..=0xdbff).contains(&text[p - 1])
                    && (0xdc00..=0xdfff).contains(&text[p])
            };
            if inside_pair(start) {
                start -= 1;
            }
            if inside_pair(end) {
                end += 1;
            }
            if start == end {
                start = crate::editing::previous_word_start(&text, start);
            }
            if start < end {
                SendMessageW(hwnd, EM_SETSEL, start, end as isize);
                SendMessageW(hwnd, EM_REPLACESEL, 1, wide("").as_ptr() as isize);
            }
            0
        }
        WM_NCDESTROY => {
            RemoveWindowSubclass(hwnd, Some(edit_proc), id);
            DefSubclassProc(hwnd, msg, w, l)
        }
        _ => DefSubclassProc(hwnd, msg, w, l),
    }
}

unsafe extern "system" fn slider_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            render::buffered(hwnd, BACKGROUND, |dc| draw_slider(hwnd, dc));
            0
        }
        WM_PRINTCLIENT => {
            let mut bounds: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut bounds);
            render::fill(w as HDC, &bounds, BACKGROUND);
            draw_slider(hwnd, w as HDC);
            0
        }
        WM_NCDESTROY => {
            RemoveWindowSubclass(hwnd, Some(slider_proc), id);
            DefSubclassProc(hwnd, msg, w, l)
        }
        _ => DefSubclassProc(hwnd, msg, w, l),
    }
}
unsafe fn draw_slider(hwnd: HWND, dc: HDC) {
    let mut channel: RECT = std::mem::zeroed();
    let mut thumb: RECT = std::mem::zeroed();
    SendMessageW(
        hwnd,
        TBM_GETCHANNELRECT,
        0,
        &mut channel as *mut RECT as isize,
    );
    SendMessageW(hwnd, TBM_GETTHUMBRECT, 0, &mut thumb as *mut RECT as isize);
    render::fill(dc, &channel, render::rgb(65, 81, 105));
    let progress = RECT {
        right: (thumb.left + thumb.right) / 2,
        ..channel
    };
    render::fill(dc, &progress, render::rgb(115, 191, 182));
    let color = if IsWindowEnabled(hwnd) == 0 {
        MUTED
    } else {
        TEXT
    };
    let brush = CreateSolidBrush(color);
    let old_brush = SelectObject(dc, brush);
    let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
    RoundRect(dc, thumb.left, thumb.top, thumb.right, thumb.bottom, 5, 5);
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(brush);
    if GetFocus() == hwnd {
        let mut bounds: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut bounds);
        InflateRect(&mut bounds, -1, -1);
        DrawFocusRect(dc, &bounds);
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use std::ptr::{null, null_mut};
    #[test]
    #[ignore = "hidden native EDIT shortcuts; run on Windows CI"]
    fn text_shortcuts_select_all_delete_words_and_preserve_native_undo() {
        std::thread::spawn(|| unsafe {
            let parent = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("").as_ptr(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                400,
                200,
                null_mut(),
                null_mut(),
                null_mut(),
                null(),
            );
            assert!(!parent.is_null());
            install();
            let edit = CreateWindowExW(
                0,
                wide("EDIT").as_ptr(),
                wide("first second 😀").as_ptr(),
                WS_CHILD | ES_AUTOHSCROLL as u32,
                0,
                0,
                300,
                30,
                parent,
                null_mut(),
                null_mut(),
                null(),
            );
            assert!(!edit.is_null());
            control(edit, "EDIT");
            let get = || {
                let mut text = vec![0u16; GetWindowTextLengthW(edit) as usize + 1];
                let count = GetWindowTextW(edit, text.as_mut_ptr(), text.len() as i32);
                String::from_utf16(&text[..count as usize]).unwrap()
            };
            // WM_CHAR is local to this hidden fixture, not system input.
            SendMessageW(edit, WM_CHAR, 1, 0);
            let (mut start, mut end) = (0u32, 0u32);
            SendMessageW(
                edit,
                EM_GETSEL,
                &mut start as *mut _ as usize,
                &mut end as *mut _ as isize,
            );
            assert_eq!(
                (start, end),
                (0, "first second 😀".encode_utf16().count() as u32)
            );
            SendMessageW(edit, WM_CHAR, 0x7f, 0);
            assert_eq!(get(), "");
            assert_ne!(SendMessageW(edit, EM_UNDO, 0, 0), 0);
            assert_eq!(get(), "first second 😀");
            SendMessageW(edit, EM_SETSEL, end as usize, end as isize);
            SendMessageW(edit, WM_CHAR, 0x7f, 0);
            assert_eq!(get(), "first second ");
            SendMessageW(edit, WM_CHAR, 0x7f, 0);
            assert_eq!(get(), "first ");
            SendMessageW(edit, EM_SETSEL, 0, 5);
            SendMessageW(edit, WM_CHAR, 0x7f, 0);
            assert_eq!(get(), " ");
            SendMessageW(edit, WM_CHAR, 8, 0);
            assert_eq!(get(), " ", "Backspace at the beginning must remain a no-op");
            SetWindowTextW(edit, wide("Read only").as_ptr());
            SendMessageW(edit, EM_SETREADONLY, 1, 0);
            SendMessageW(edit, WM_CHAR, 1, 0);
            SendMessageW(edit, WM_CHAR, 0x7f, 0);
            assert_eq!(get(), "Read only");
            assert_eq!(IsWindowVisible(parent), 0);
            DestroyWindow(parent);
            shutdown();
        })
        .join()
        .unwrap();
    }
}
