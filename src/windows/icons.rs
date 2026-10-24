//! Embedded, shared Windows icon handles: no asset files loaded at runtime.
use windows_sys::Win32::{System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*};

// Three smaller four-point stars, arranged as a compact asymmetric cluster.
pub const CLUSTER: [(f64, f64, f64); 3] = [(-6., 1., 4.5), (5., -5., 3.8), (6., 6., 3.)];

pub unsafe fn small() -> HICON {
    unsafe {
        LoadImageW(
            GetModuleHandleW(std::ptr::null()),
            101usize as _,
            IMAGE_ICON,
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
            LR_SHARED,
        )
        .cast()
    }
}
pub unsafe fn large() -> HICON {
    unsafe { LoadIconW(GetModuleHandleW(std::ptr::null()), 101usize as _) }
}

pub unsafe fn set_small(hwnd: windows_sys::Win32::Foundation::HWND) {
    if !hwnd.is_null() {
        unsafe {
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small() as isize);
        }
    }
}
