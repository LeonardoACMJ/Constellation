//! Editor-only native resources. Nothing is allocated in the tray process.
#![allow(unsafe_op_in_unsafe_fn)]
use super::{render, wide};
use std::cell::{Cell, RefCell};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    UI::{
        Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
        Controls::*,
        Input::KeyboardAndMouse::{GetFocus, IsWindowEnabled},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};
pub const DISPLAY_CHANGED: u32 = WM_APP + 64;
thread_local! {
    static DISPLAY:RefCell<std::collections::HashMap<usize,(RECT,u32,u32)>>=RefCell::new(std::collections::HashMap::new());
}
/// Copy the suggested rectangle while its native message is live. The posted
/// message carries no pointer and enters layout through the normal guard.
pub unsafe fn display_message(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> Option<LRESULT> {
    if msg == WM_DPICHANGED && l != 0 {
        let rect = *(l as *const RECT);
        let old = GetPropW(hwnd, wide("Constellation.EditorDpi").as_ptr()) as u32;
        DISPLAY.with(|items| {
            items
                .borrow_mut()
                .insert(hwnd as usize, (rect, old.max(96), (w & 0xffff) as u32))
        });
        PostMessageW(hwnd, DISPLAY_CHANGED, 0, 0);
        return Some(0);
    }
    if msg == WM_SETTINGCHANGE {
        PostMessageW(hwnd, DISPLAY_CHANGED, 0, 0);
    }
    if msg == DISPLAY_CHANGED {
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, class.as_mut_ptr(), 64).max(0) as usize;
        if String::from_utf16_lossy(&class[..n]) != "Constellation.Config" {
            if let Some(guard) = super::DispatchGuard::enter() {
                apply_display(hwnd, SOURCE.with(|p| p.get()), true);
                drop(guard);
            } else {
                super::defer_message(hwnd, msg, w, l);
            }
            return Some(0);
        }
    }
    None
}
pub unsafe fn apply_display(hwnd: HWND, palette: crate::theme::Palette, scale_children: bool) {
    let change = DISPLAY.with(|items| items.borrow_mut().remove(&(hwnd as usize)));
    if let Some((rect, old, new)) = change {
        if scale_children {
            let mut child = GetWindow(hwnd, GW_CHILD);
            while !child.is_null() {
                let next = GetWindow(child, GW_HWNDNEXT);
                let mut r: RECT = std::mem::zeroed();
                GetWindowRect(child, &mut r);
                MapWindowPoints(std::ptr::null_mut(), hwnd, (&mut r as *mut RECT).cast(), 2);
                let px = |n: i32| (n as f64 * new as f64 / old as f64).round() as i32;
                SetWindowPos(
                    child,
                    std::ptr::null_mut(),
                    px(r.left),
                    px(r.top),
                    px(r.right - r.left),
                    px(r.bottom - r.top),
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOREDRAW,
                );
                child = next;
            }
        }
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    SetPropW(
        hwnd,
        wide("Constellation.EditorDpi").as_ptr(),
        windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd) as usize as HANDLE,
    );
    refresh(hwnd, palette);
}
pub unsafe fn logical_dc(dc: HDC, dpi: u32) {
    SetMapMode(dc, MM_ANISOTROPIC);
    SetWindowExtEx(dc, 96, 96, std::ptr::null_mut());
    SetViewportExtEx(dc, dpi as i32, dpi as i32, std::ptr::null_mut());
}
thread_local! {static PALETTE:Cell<crate::theme::Palette>=Cell::new(crate::theme::Palette::default());}
pub fn source() -> crate::theme::Palette {
    SOURCE.with(|p| p.get())
}
pub fn background() -> u32 {
    PALETTE.with(|p| p.get().background.gdi())
}
pub fn text_color() -> u32 {
    PALETTE.with(|p| p.get().text.gdi())
}
pub fn muted() -> u32 {
    PALETTE.with(|p| p.get().muted.gdi())
}
fn input_color() -> u32 {
    PALETTE.with(|p| p.get().input.gdi())
}
pub fn effective(mut palette: crate::theme::Palette) -> crate::theme::Palette {
    unsafe {
        let mut hc: HIGHCONTRASTW = std::mem::zeroed();
        hc.cbSize = std::mem::size_of::<HIGHCONTRASTW>() as u32;
        if SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cbSize,
            (&mut hc as *mut HIGHCONTRASTW).cast(),
            0,
        ) != 0
            && hc.dwFlags & HCF_HIGHCONTRASTON != 0
        {
            let from = |index| {
                let c = GetSysColor(index);
                crate::theme::Color(((c & 255) << 16) | (c & 0xff00) | (c >> 16))
            };
            palette.background = from(COLOR_WINDOW);
            palette.input = from(COLOR_WINDOW);
            palette.text = from(COLOR_WINDOWTEXT);
            palette.muted = from(COLOR_WINDOWTEXT);
            palette.star = from(COLOR_WINDOW);
            palette.constellation = from(COLOR_WINDOW);
            palette.star_highlight = from(COLOR_HIGHLIGHT);
            palette.constellation_highlight = from(COLOR_HIGHLIGHT);
            palette.outline = from(COLOR_WINDOWTEXT);
            palette.line = from(COLOR_WINDOWTEXT);
            palette.symbol = from(COLOR_WINDOWTEXT);
            palette.cluster = from(COLOR_WINDOWTEXT);
            palette.hub = from(COLOR_WINDOW);
        }
        palette
    }
}
struct Theme {
    background: HBRUSH,
    input: HBRUSH,
}
impl Drop for Theme {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.background);
            DeleteObject(self.input);
        }
    }
}
thread_local! {static FONTS:RefCell<std::collections::HashMap<u32,HFONT>>=RefCell::new(std::collections::HashMap::new());}
thread_local! {static SOURCE:Cell<crate::theme::Palette>=Cell::new(crate::theme::Palette::default());}
thread_local! {static THEME:RefCell<Option<Theme>>=const {RefCell::new(None)};}
pub unsafe fn install() {
    install_at(crate::theme::Palette::default(), 96);
}
pub unsafe fn install_at(palette: crate::theme::Palette, dpi: u32) {
    PALETTE.with(|p| p.set(effective(palette)));
    SOURCE.with(|p| p.set(palette));
    let _ = font_for(dpi);
    THEME.with(|v| {
        *v.borrow_mut() = Some(Theme {
            background: CreateSolidBrush(background()),
            input: CreateSolidBrush(input_color()),
        })
    });
}
unsafe fn font_for(dpi: u32) -> HFONT {
    FONTS.with(|fonts| {
        *fonts.borrow_mut().entry(dpi).or_insert_with(|| {
            let font = CreateFontW(
                -(16. * dpi as f64 / 96.).round() as i32,
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
            font
        })
    })
}
pub unsafe fn refresh(hwnd: HWND, palette: crate::theme::Palette) {
    install_at(
        palette,
        windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd),
    );
    unsafe extern "system" fn visit(hwnd: HWND, _: LPARAM) -> i32 {
        unsafe {
            let mut class = [0u16; 64];
            let n = GetClassNameW(hwnd, class.as_mut_ptr(), 64).max(0) as usize;
            control(hwnd, &String::from_utf16_lossy(&class[..n]));
        }
        1
    }
    window(hwnd);
    EnumChildWindows(hwnd, Some(visit), 0);
    RedrawWindow(
        hwnd,
        std::ptr::null(),
        std::ptr::null_mut(),
        RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_NOERASE,
    );
}
pub fn shutdown() {
    THEME.with(|v| v.borrow_mut().take());
    FONTS.with(|fonts| {
        for (_, font) in fonts.borrow_mut().drain() {
            unsafe {
                DeleteObject(font);
            }
        }
    });
}
pub unsafe fn window(hwnd: HWND) {
    SetPropW(
        hwnd,
        wide("Constellation.EditorDpi").as_ptr(),
        windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd) as usize as HANDLE,
    );
    let dark = i32::from(PALETTE.with(|p| p.get().background.0 & 0xffffff) < 0x808080);
    DwmSetWindowAttribute(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
        &dark as *const _ as _,
        4,
    );
}
pub unsafe fn control(hwnd: HWND, class: &str) {
    let font = font_for(windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd));
    SendMessageW(hwnd, WM_SETFONT, font as usize, 0);
    if class == "EDIT" {
        SetWindowSubclass(hwnd, Some(edit_proc), 2, 0);
    }
    if class == "SysTreeView32" {
        SetWindowTheme(hwnd, wide("").as_ptr(), wide("").as_ptr());
        SendMessageW(hwnd, TVM_SETBKCOLOR, 0, input_color() as isize);
        SendMessageW(hwnd, TVM_SETTEXTCOLOR, 0, text_color() as isize);
        SendMessageW(hwnd, TVM_SETLINECOLOR, 0, render::rgb(65, 81, 105) as isize);
        SendMessageW(hwnd, TVM_SETINDENT, 20, 0);
        SendMessageW(
            hwnd,
            TVM_SETITEMHEIGHT,
            (24. * windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd) as f64 / 96.).round()
                as usize,
            0,
        );
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
                SetTextColor(dc, text_color());
                SetBkColor(dc, if input { input_color() } else { background() });
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
        PALETTE.with(|p| p.get().star_highlight.gdi())
    } else {
        input_color()
    };
    render::fill(dc, &r, background());
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
    let old = SelectObject(dc, SendMessageW(item.hwndItem, WM_GETFONT, 0, 0) as HFONT);
    SetTextColor(dc, if disabled { muted() } else { text_color() });
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
    SelectObject(dc, old);
}
pub unsafe fn panel(dc: HDC, r: RECT) {
    let pen = CreatePen(PS_SOLID, 1, PALETTE.with(|p| p.get().outline.gdi()));
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
    let undo_state = wide("Constellation.Edit.Undone");
    match msg {
        WM_CHAR if matches!(w, 0x1a | 0x19) => {
            if GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & ES_READONLY as u32 != 0
                || IsWindowEnabled(hwnd) == 0
            {
                return 0;
            }
            // Classic EDIT keeps a single toggling undo record. Ctrl+Y may
            // toggle it only after Undo, never undo a fresh edit by accident.
            if w == 0x1a || !GetPropW(hwnd, undo_state.as_ptr()).is_null() {
                SendMessageW(hwnd, EM_UNDO, 0, 0);
            }
            0
        }
        WM_UNDO | EM_UNDO => {
            let undone = !GetPropW(hwnd, undo_state.as_ptr()).is_null();
            let result = DefSubclassProc(hwnd, msg, w, l);
            if result != 0 {
                if undone {
                    RemovePropW(hwnd, undo_state.as_ptr());
                } else {
                    SetPropW(hwnd, undo_state.as_ptr(), 1usize as HANDLE);
                }
            }
            result
        }
        WM_SETTEXT | WM_CUT | WM_CLEAR | WM_PASTE | EM_REPLACESEL => {
            RemovePropW(hwnd, undo_state.as_ptr());
            DefSubclassProc(hwnd, msg, w, l)
        }
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
        WM_CHAR if w >= 32 || matches!(w, 8 | 9 | 13) => {
            RemovePropW(hwnd, undo_state.as_ptr());
            DefSubclassProc(hwnd, msg, w, l)
        }
        WM_NCDESTROY => {
            RemovePropW(hwnd, undo_state.as_ptr());
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
            render::buffered(hwnd, background(), |dc| draw_slider(hwnd, dc));
            0
        }
        WM_PRINTCLIENT => {
            let mut bounds: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut bounds);
            render::fill(w as HDC, &bounds, background());
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
        muted()
    } else {
        text_color()
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
            SetWindowTextW(edit, wide("base").as_ptr());
            SendMessageW(edit, EM_SETSEL, 4, 4);
            SendMessageW(edit, EM_REPLACESEL, 1, wide(" tail").as_ptr() as isize);
            assert_eq!(get(), "base tail");
            SendMessageW(edit, WM_CHAR, 0x1a, 0);
            assert_eq!(get(), "base");
            SendMessageW(edit, WM_CHAR, 0x19, 0);
            assert_eq!(get(), "base tail");
            SendMessageW(edit, WM_CHAR, 0x19, 0);
            assert_eq!(get(), "base tail", "Redo cannot undo a fresh/restored edit");
            SendMessageW(edit, WM_CHAR, 0x1a, 0);
            assert_eq!(get(), "base");
            SendMessageW(edit, EM_REPLACESEL, 1, wide(" new").as_ptr() as isize);
            let fresh = get();
            SendMessageW(edit, WM_CHAR, 0x19, 0);
            assert_eq!(get(), fresh, "New editing clears Redo");
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
