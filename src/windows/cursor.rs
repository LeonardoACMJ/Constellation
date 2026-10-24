//! Window-local native pointer. Never replaces a Windows system cursor or
//! changes ShowCursor counters, mouse capture, clipping or foreground focus.
#![allow(unsafe_op_in_unsafe_fn)]
use std::{
    mem::{size_of, zeroed},
    ptr::null_mut,
};
use windows_sys::Win32::{Graphics::Gdi::*, UI::WindowsAndMessaging::*};
thread_local! {
    static ACTIVE: std::cell::Cell<HCURSOR> = const { std::cell::Cell::new(null_mut()) };
}

// WM_SETCURSOR can arrive synchronously during drawing/window operations.
// Serve it without borrowing the Runtime again or falling back to an arrow.
pub unsafe fn set_current() -> bool {
    let handle = ACTIVE.with(std::cell::Cell::get);
    if handle.is_null() {
        return false;
    }
    SetCursor(handle);
    true
}

fn pixels(size: usize, scale: f64) -> Vec<u8> {
    let center = (size / 2) as f64;
    let mut result = vec![0; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let distance = (x as f64 - center).hypot(y as f64 - center);
            let outer = (4. * scale + 0.5 - distance).clamp(0., 1.);
            let inner = (3. * scale + 0.5 - distance).clamp(0., 1.);
            let i = (y * size + x) * 4;
            for (channel, dark) in [39., 25., 18.].into_iter().enumerate() {
                result[i + channel] = (255. * inner + dark * (outer - inner)).round() as u8;
            }
            result[i + 3] = (255. * outer).round() as u8;
        }
    }
    result
}

unsafe fn create(scale: f64) -> HCURSOR {
    let size = (32. * scale).round().clamp(16., 128.) as i32;
    let image = pixels(size as usize, scale);
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            biHeight: -size,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..zeroed()
        },
        ..zeroed()
    };
    let mut data = null_mut();
    let color = CreateDIBSection(null_mut(), &info, DIB_RGB_COLORS, &mut data, null_mut(), 0);
    if color.is_null() || data.is_null() {
        if !color.is_null() {
            DeleteObject(color);
        }
        return null_mut();
    }
    std::ptr::copy_nonoverlapping(image.as_ptr(), data.cast(), image.len());
    let stride = (size as usize).div_ceil(16) * 2;
    let mut mask = vec![255u8; stride * size as usize];
    for y in 0..size as usize {
        for x in 0..size as usize {
            if image[(y * size as usize + x) * 4 + 3] > 0 {
                mask[y * stride + x / 8] &= !(0x80 >> (x % 8));
            }
        }
    }
    let mask = CreateBitmap(size, size, 1, 1, mask.as_ptr().cast());
    let cursor = if mask.is_null() {
        null_mut()
    } else {
        CreateIconIndirect(&ICONINFO {
            fIcon: 0,
            xHotspot: (size / 2) as u32,
            yHotspot: (size / 2) as u32,
            hbmMask: mask,
            hbmColor: color,
        })
    };
    if !mask.is_null() {
        DeleteObject(mask);
    }
    DeleteObject(color);
    cursor
}

#[derive(Default)]
pub struct DotCursor {
    handle: HCURSOR,
    scale: f64,
    previous: Option<HCURSOR>,
    pub enabled: bool,
}
impl DotCursor {
    pub unsafe fn prepare(&mut self, scale: f64) {
        if self.handle.is_null() || self.scale != scale {
            self.restore();
            if !self.handle.is_null() {
                DestroyCursor(self.handle);
            }
            self.handle = create(scale);
            self.scale = scale;
        }
        // Allocation failure leaves the standard pointer usable.
        self.enabled = !self.handle.is_null();
    }
    pub unsafe fn apply(&mut self) {
        if self.enabled {
            let previous = SetCursor(self.handle);
            if self.previous.is_none() {
                self.previous = Some(previous);
            }
            ACTIVE.with(|value| value.set(self.handle));
        }
    }
    pub unsafe fn restore(&mut self) {
        self.enabled = false;
        ACTIVE.with(|value| {
            if value.get() == self.handle {
                value.set(null_mut());
            }
        });
        if let Some(previous) = self.previous.take() {
            // Another app may already have installed its own pointer during
            // cancellation. Never overwrite that newer cursor.
            if GetCursor() == self.handle {
                SetCursor(if previous.is_null() {
                    LoadCursorW(null_mut(), IDC_ARROW)
                } else {
                    previous
                });
            }
        }
    }
}
impl Drop for DotCursor {
    fn drop(&mut self) {
        unsafe {
            self.restore();
            if !self.handle.is_null() {
                DestroyCursor(self.handle);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dot_has_centered_white_hotspot_and_premultiplied_antialiased_edges() {
        for scale in [1., 1.25, 1.5, 2., 3.] {
            let size = (32. * scale) as usize;
            let image = pixels(size, scale);
            let center = (size / 2 * size + size / 2) * 4;
            assert_eq!(&image[center..center + 4], &[255; 4]);
            assert_eq!(&image[..4], &[0; 4]);
            assert!(image.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255));
            for p in image.chunks_exact(4) {
                assert!(p[..3].iter().all(|channel| *channel <= p[3]));
            }
        }
    }
}
