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

pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
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

unsafe fn symbol(dc: HDC, p: Point, folder: bool, scale: f64, palette: crate::theme::Palette) {
    let shapes: &[(f64, f64, f64)] = if folder {
        &super::icons::CLUSTER
    } else {
        &[(0., 0., 7.5)]
    };
    let brush = CreateSolidBrush(if folder {
        palette.cluster.gdi()
    } else {
        palette.symbol.gdi()
    });
    let old_brush = SelectObject(dc, brush);
    let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
    for &(x, y, radius) in shapes {
        let center = Point {
            x: p.x + x * scale,
            y: p.y + y * scale,
        };
        let points: Vec<POINT> = (0..8)
            .map(|i| {
                let q = center.polar(
                    radius * scale * if i % 2 == 0 { 1. } else { 0.25 },
                    -std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::FRAC_PI_4,
                );
                POINT {
                    x: q.x.round() as i32,
                    y: q.y.round() as i32,
                }
            })
            .collect();
        Polygon(dc, points.as_ptr(), points.len() as i32);
    }
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(brush);
}

#[cfg(test)]
pub unsafe fn draw(
    dc: HDC,
    c: &Config,
    start: &crate::config::Node,
    sectors: &[Sector],
    path: &[u64],
    hover: Option<u64>,
    origin: Point,
    scale: f64,
    preview: bool,
) {
    draw_with_icons(
        dc,
        c,
        start,
        sectors,
        path,
        hover,
        origin,
        scale,
        preview,
        &super::item_icons::Images::new(),
    );
}
/// `start` is the node the radial opened at: a universe root, or a shortcut
/// profile's own entry node.
pub unsafe fn draw_with_icons(
    dc: HDC,
    c: &Config,
    start: &crate::config::Node,
    sectors: &[Sector],
    path: &[u64],
    hover: Option<u64>,
    origin: Point,
    scale: f64,
    preview: bool,
    images: &super::item_icons::Images,
) {
    let map = |p: Point| Point {
        x: origin.x + p.x * scale,
        y: origin.y + p.y * scale,
    };
    fn index<'a>(
        node: &'a crate::config::Node,
        nodes: &mut std::collections::HashMap<u64, &'a crate::config::Node>,
        parents: &mut std::collections::HashMap<u64, u64>,
    ) {
        nodes.insert(node.id, node);
        if let Some(children) = &node.children {
            for child in children {
                parents.insert(child.id, node.id);
                index(child, nodes, parents);
            }
        }
    }
    let mut nodes = std::collections::HashMap::new();
    let mut parents = std::collections::HashMap::new();
    index(start, &mut nodes, &mut parents);
    let palette = super::editor_theme::effective(c.theme);
    let brushes = [
        palette.star.gdi(),
        palette.constellation.gdi(),
        palette.star_highlight.gdi(),
        palette.constellation_highlight.gdi(),
    ]
    .map(|v| CreateSolidBrush(v));
    let pens = palette
        .outline_colors()
        .map(|color| CreatePen(PS_SOLID, 3, color.gdi()));
    for s in sectors {
        let Some(node) = nodes.get(&s.id) else {
            continue;
        };
        let active = hover == Some(s.id) || path.contains(&s.id);
        let folder = node.children.is_some();
        let brush = brushes[usize::from(folder) + 2 * usize::from(active)];
        let pen = pens[usize::from(folder) + 2 * usize::from(active)];
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
        let arc = (s.inner + s.outer) * 0.5 * s.sweep * scale;
        if arc < 18. || (s.outer - s.inner) * scale < 18. {
            continue;
        }
        let p = map(s.dot);
        let w = (((s.inner + s.outer) * 0.5 * s.sweep * scale) as i32).clamp(12, 125);
        let size = (12. * scale).round().max(7.) as i32;
        if let Some(image) = images.get(&node.id) {
            let size = (24. * scale).round().max(1.) as i32;
            super::item_icons::draw_gdi(
                dc,
                image,
                p.x.round() as i32 - size / 2,
                (p.y - 15. * scale).round() as i32 - size / 2,
                size,
            );
        } else {
            symbol(
                dc,
                Point {
                    x: p.x,
                    y: p.y - 15. * scale,
                },
                folder,
                scale,
                palette,
            );
        }
        text(
            dc,
            &node.name,
            p.x as i32 - w / 2,
            p.y as i32 + 3,
            w,
            palette.text.gdi(),
            size,
        );
    }
    for handle in brushes {
        DeleteObject(handle);
    }
    for handle in pens {
        DeleteObject(handle);
    }
    let lit: Vec<_> = sectors
        .iter()
        .filter(|s| preview || path.contains(&s.id) || hover == Some(s.id))
        .collect();
    let lit_by_id: std::collections::HashMap<_, _> = lit.iter().map(|s| (s.id, *s)).collect();
    for child in &lit {
        if child.depth == 0 {
            continue;
        }
        let parent = parents.get(&child.id).and_then(|id| lit_by_id.get(id));
        if let Some(parent) = parent {
            line(
                dc,
                map(parent.dot),
                map(child.dot),
                if preview { 1 } else { 2 },
                palette.line.gdi(),
            );
        }
    }
    for s in lit {
        let p = map(s.dot);
        dot(dc, p, 6, rgb(76, 98, 142));
        dot(dc, p, 3, rgb(235, 250, 255));
    }
    let current = path.last().and_then(|id| c.find(*id)).unwrap_or(start);
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
    dot(dc, origin, radius, palette.hub.gdi());
    if let Some(image) = images.get(&start.id) {
        let size = (24. * scale).round().max(1.) as i32;
        super::item_icons::draw_gdi(
            dc,
            image,
            origin.x.round() as i32 - size / 2,
            (origin.y - 7. * scale).round() as i32 - size / 2,
            size,
        );
    } else {
        symbol(
            dc,
            Point {
                x: origin.x,
                y: origin.y - 7. * scale,
            },
            true,
            scale,
            palette,
        );
    }
    if radius > 25 {
        text(
            dc,
            "ROOT",
            origin.x as i32 - 28,
            origin.y as i32 + 7,
            56,
            palette.muted.gdi(),
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
    buffer_into(hwnd, dc, background, paint);
    EndPaint(hwnd, &ps);
}
unsafe fn buffer_into(
    hwnd: windows_sys::Win32::Foundation::HWND,
    dc: HDC,
    background: u32,
    paint: impl FnOnce(HDC),
) {
    let mut r: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut r);
    let memory = CreateCompatibleDC(dc);
    let bitmap = CreateCompatibleBitmap(dc, r.right.max(1), r.bottom.max(1));
    let old = SelectObject(memory, bitmap);
    fill(memory, &r, background);
    let state = SaveDC(memory);
    paint(memory);
    // Paint callbacks may translate/clip their content. The final copy uses
    // device pixels, never the callback's scrolled logical coordinate system.
    RestoreDC(memory, state);
    BitBlt(dc, 0, 0, r.right, r.bottom, memory, 0, 0, SRCCOPY);
    SelectObject(memory, old);
    DeleteObject(bitmap);
    DeleteDC(memory);
}
