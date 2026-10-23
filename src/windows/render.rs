#![allow(unsafe_op_in_unsafe_fn)]
use super::wide;
use crate::{
    config::Config,
    geometry::{Point, Sector},
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows_sys::Win32::{
    Foundation::{POINT, RECT},
    Graphics::Gdi::*,
};

pub fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}
pub unsafe fn fill(dc: HDC, rect: &RECT, color: u32) {
    let brush = CreateSolidBrush(color);
    FillRect(dc, rect, brush);
    DeleteObject(brush);
}
pub unsafe fn text(dc: HDC, s: &str, x: i32, y: i32, width: i32, color: u32, size: i32) {
    let font = CreateFontW(
        size,
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
    let old = SelectObject(dc, font);
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, color);
    let mut rect = RECT {
        left: x,
        top: y,
        right: x + width,
        bottom: y + size + 6,
    };
    let string = wide(s);
    DrawTextW(
        dc,
        string.as_ptr(),
        (string.len() - 1) as i32,
        &mut rect,
        DT_CENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
    SelectObject(dc, old);
    DeleteObject(font);
}
unsafe fn line(dc: HDC, a: Point, b: Point, width: i32, color: u32) {
    let pen = CreatePen(PS_SOLID, width, color);
    let old = SelectObject(dc, pen);
    MoveToEx(dc, a.x as i32, a.y as i32, std::ptr::null_mut());
    LineTo(dc, b.x as i32, b.y as i32);
    SelectObject(dc, old);
    DeleteObject(pen);
}
unsafe fn dot(dc: HDC, p: Point, r: i32, color: u32) {
    let brush = CreateSolidBrush(color);
    let old = SelectObject(dc, brush);
    let oldpen = SelectObject(dc, GetStockObject(NULL_PEN));
    Ellipse(
        dc,
        p.x as i32 - r,
        p.y as i32 - r,
        p.x as i32 + r + 1,
        p.y as i32 + r + 1,
    );
    SelectObject(dc, old);
    SelectObject(dc, oldpen);
    DeleteObject(brush);
}

pub unsafe fn draw(
    dc: HDC,
    c: &Config,
    sectors: &[Sector],
    path: &[u64],
    hover: Option<u64>,
    origin: Point,
    scale: f64,
    preview: bool,
) {
    let map = |p: Point| Point {
        x: origin.x + p.x * scale,
        y: origin.y + p.y * scale,
    };
    for s in sectors {
        let Some(node) = c.find(s.id) else { continue };
        let active = hover == Some(s.id) || path.contains(&s.id);
        let folder = node.children.is_some();
        let color = if active {
            if folder {
                rgb(89, 69, 131)
            } else {
                rgb(40, 100, 102)
            }
        } else if folder {
            rgb(43, 38, 67)
        } else {
            rgb(28, 43, 59)
        };
        let brush = CreateSolidBrush(color);
        let pen = CreatePen(
            PS_SOLID,
            1,
            if active {
                rgb(203, 184, 255)
            } else {
                rgb(77, 89, 117)
            },
        );
        let ob = SelectObject(dc, brush);
        let op = SelectObject(dc, pen);
        let polygon: Vec<POINT> = s
            .polygon()
            .into_iter()
            .map(|p| {
                let p = map(p);
                POINT {
                    x: p.x.round() as i32,
                    y: p.y.round() as i32,
                }
            })
            .collect();
        if s.sweep > 0. {
            Polygon(dc, polygon.as_ptr(), polygon.len() as i32);
        }
        SelectObject(dc, ob);
        SelectObject(dc, op);
        DeleteObject(brush);
        DeleteObject(pen);
        let p = map(s.dot);
        let w = (((s.inner + s.outer) * 0.5 * s.sweep * scale) as i32).clamp(12, 125);
        let size = (12. * scale).round().max(7.) as i32;
        text(
            dc,
            if folder { "✧" } else { "✦" },
            p.x as i32 - 15,
            p.y as i32 - 26,
            30,
            if folder {
                rgb(195, 175, 255)
            } else {
                rgb(142, 226, 205)
            },
            (21. * scale).max(9.) as i32,
        );
        text(
            dc,
            &node.name,
            p.x as i32 - w / 2,
            p.y as i32 + 3,
            w,
            rgb(233, 237, 250),
            size,
        );
    }
    let lit: Vec<_> = sectors
        .iter()
        .filter(|s| preview || path.contains(&s.id) || hover == Some(s.id))
        .collect();
    for child in &lit {
        if child.depth == 0 {
            continue;
        }
        let parent = lit.iter().find(|parent| {
            parent.depth + 1 == child.depth
                && c.find(parent.id)
                    .and_then(|n| n.children.as_ref())
                    .is_some_and(|cs| cs.iter().any(|n| n.id == child.id))
        });
        if let Some(parent) = parent {
            line(
                dc,
                map(parent.dot),
                map(child.dot),
                if preview { 1 } else { 2 },
                rgb(172, 210, 247),
            );
        }
    }
    for s in lit {
        let p = map(s.dot);
        dot(dc, p, 6, rgb(76, 98, 142));
        dot(dc, p, 3, rgb(235, 250, 255));
    }
    let current = path.last().and_then(|id| c.find(*id)).unwrap_or(&c.root);
    if !preview
        && current
            .children
            .as_ref()
            .is_some_and(|children| children.is_empty())
    {
        text(
            dc,
            &format!("{} is empty · Backspace or center to return", current.name),
            origin.x as i32 - 230,
            origin.y as i32 + (c.layout.radius * scale) as i32 + 10,
            460,
            rgb(205, 215, 235),
            13,
        );
    }
    let radius = (c.layout.radius * 0.27 * scale).max(3.) as i32;
    dot(dc, origin, radius, rgb(18, 25, 39));
    text(
        dc,
        "✧",
        origin.x as i32 - 20,
        origin.y as i32 - 23,
        40,
        rgb(195, 175, 255),
        26,
    );
    if radius > 25 {
        text(
            dc,
            "ROOT",
            origin.x as i32 - 28,
            origin.y as i32 + 7,
            56,
            rgb(156, 175, 207),
            10,
        );
    }
}

pub unsafe fn buffered(
    hwnd: windows_sys::Win32::Foundation::HWND,
    background: u32,
    paint: impl FnOnce(HDC),
) {
    let mut ps = std::mem::zeroed();
    let dc = BeginPaint(hwnd, &mut ps);
    let mut r: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut r);
    let memory = CreateCompatibleDC(dc);
    let bitmap = CreateCompatibleBitmap(dc, r.right.max(1), r.bottom.max(1));
    let old = SelectObject(memory, bitmap);
    fill(memory, &r, background);
    paint(memory);
    BitBlt(dc, 0, 0, r.right, r.bottom, memory, 0, 0, SRCCOPY);
    SelectObject(memory, old);
    DeleteObject(bitmap);
    DeleteDC(memory);
    EndPaint(hwnd, &ps);
}
