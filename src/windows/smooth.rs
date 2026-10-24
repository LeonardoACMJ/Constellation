//! Analytic, antialiased radial drawing into premultiplied BGRA, composed by
//! Windows with per-pixel alpha. No color-key matte or render/animation thread.
#![allow(unsafe_op_in_unsafe_fn)]
use super::wide;
use crate::{
    config::Config,
    geometry::{Point, Sector},
};
use std::{
    collections::HashMap,
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{HWND, POINT, SIZE},
    Graphics::{Gdi::*, GdiPlus::*},
    UI::WindowsAndMessaging::{GetClientRect, ULW_ALPHA, UpdateLayeredWindow},
};

const PARGB: i32 = (11 | (32 << 8) | PixelFormatGDI | PixelFormatAlpha | PixelFormatPAlpha) as i32;
fn status(value: Status) -> Result<(), String> {
    if value == Ok {
        Result::Ok(())
    } else {
        Err(format!(
            "Smooth radial drawing failed (GDI+ status {value})"
        ))
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use std::path::Path;
    #[test]
    #[ignore = "offscreen Windows icon extraction; remote CI only"]
    fn automatic_and_custom_icons_render_with_transparency_and_missing_fallback() {
        unsafe {
            use super::super::item_icons::{self, Source};
            let com = windows_sys::Win32::System::Com::CoInitializeEx(
                null(),
                windows_sys::Win32::System::Com::COINIT_APARTMENTTHREADED as u32,
            );
            let _engine = Engine::start().unwrap();
            let root = Path::new(env!("CARGO_MANIFEST_DIR"));
            let png = item_icons::load(root, &Source::Custom("assets/app-icon.png".into()), true)
                .unwrap();
            let ico =
                item_icons::load(root, &Source::Custom("assets/app.ico".into()), true).unwrap();
            let exe = item_icons::load(
                root,
                &Source::File(
                    std::env::current_exe()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                ),
                true,
            )
            .unwrap();
            for image in [&png, &ico, &exe] {
                assert_eq!(image.bytes.len(), 64 * 64 * 4);
                assert!(image.bytes.chunks_exact(4).any(|p| p[3] == 0));
                assert!(image.bytes.chunks_exact(4).any(|p| p[3] > 0));
                assert!(
                    image
                        .bytes
                        .chunks_exact(4)
                        .all(|p| p[..3].iter().all(|c| *c <= p[3]))
                );
            }
            assert!(item_icons::load(root, &Source::Custom("missing.png".into()), true).is_none());
            let mut images = item_icons::Images::new();
            images.insert(3, std::sync::Arc::new(exe));
            images.insert(4, std::sync::Arc::new(ico));
            images.insert(5, std::sync::Arc::new(png));
            let c = Config::default();
            let path = [2, 5];
            let sectors = crate::geometry::visible(&c, &path);
            let mut surface = Surface::new(800, 800).unwrap();
            surface
                .draw_with_icons(
                    &c,
                    &sectors,
                    &path,
                    Some(7),
                    Point { x: 400., y: 400. },
                    1.25,
                    false,
                    &images,
                )
                .unwrap();
            let output = Path::new(".local/render-review");
            std::fs::create_dir_all(output).unwrap();
            save_png(
                &composite(&surface, [248, 248, 248]),
                &output.join("item-icons-light.png"),
            );
            save_png(
                &composite(&surface, [24, 20, 16]),
                &output.join("item-icons-dark.png"),
            );
            if com >= 0 {
                windows_sys::Win32::System::Com::CoUninitialize();
            }
        }
    }
    unsafe fn bytes(surface: &Surface) -> &[u8] {
        std::slice::from_raw_parts(
            surface.pixels,
            surface.width as usize * surface.height as usize * 4,
        )
    }
    unsafe fn save_png(surface: &Surface, path: &Path) {
        let mut count = 0;
        let mut size = 0;
        status(GdipGetImageEncodersSize(&mut count, &mut size)).unwrap();
        let mut storage = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
        status(GdipGetImageEncoders(
            count,
            size,
            storage.as_mut_ptr().cast(),
        ))
        .unwrap();
        let encoders =
            std::slice::from_raw_parts(storage.as_ptr().cast::<ImageCodecInfo>(), count as usize);
        let encoder = encoders
            .iter()
            .find(|encoder| {
                let mut len = 0;
                while *encoder.MimeType.add(len) != 0 {
                    len += 1;
                }
                String::from_utf16_lossy(std::slice::from_raw_parts(encoder.MimeType, len))
                    == "image/png"
            })
            .unwrap();
        let image = owned(
            |out| {
                GdipCreateBitmapFromScan0(
                    surface.width,
                    surface.height,
                    surface.width * 4,
                    PARGB,
                    surface.pixels,
                    out,
                )
            },
            |p: *mut GpBitmap| GdipDisposeImage(p.cast()),
        )
        .unwrap();
        status(GdipSaveImageToFile(
            image.pointer.cast(),
            wide(path).as_ptr(),
            &encoder.Clsid,
            null(),
        ))
        .unwrap();
    }
    unsafe fn composite(surface: &Surface, background: [u8; 3]) -> Surface {
        let output = Surface::new(surface.width, surface.height).unwrap();
        let target = std::slice::from_raw_parts_mut(output.pixels, bytes(surface).len());
        for (source, target) in bytes(surface)
            .chunks_exact(4)
            .zip(target.chunks_exact_mut(4))
        {
            for channel in 0..3 {
                target[channel] = (source[channel] as u16
                    + (background[channel] as u16 * (255 - source[3] as u16) + 127) / 255)
                    .min(255) as u8;
            }
            target[3] = 255;
        }
        output
    }
    #[test]
    #[ignore = "offscreen Windows vector rendering; remote CI only"]
    fn antialiasing_is_premultiplied_stable_and_has_no_color_key_matte() {
        unsafe {
            let _engine = Engine::start().unwrap();
            let c = Config::default();
            let path = [2, 5];
            let sectors = crate::geometry::visible(&c, &path);
            let origin = Point { x: 400., y: 400. };
            let mut surface = Surface::new(800, 800).unwrap();
            let started = std::time::Instant::now();
            surface
                .draw(&c, &sectors, &path, Some(7), origin, 1.25, false)
                .unwrap();
            let first_ms = started.elapsed().as_secs_f64() * 1000.;
            let first = bytes(&surface).to_vec();
            let mut levels = std::collections::BTreeSet::new();
            let mut partial = 0;
            let mut transparent = 0;
            for pixel in first.chunks_exact(4) {
                assert!(
                    pixel[..3].iter().all(|channel| *channel <= pixel[3]),
                    "RGB must be premultiplied by alpha"
                );
                if pixel[3] == 0 {
                    transparent += 1;
                    assert_eq!(&pixel[..3], &[0, 0, 0]);
                }
                if pixel[3] > 0 && pixel[3] < 255 {
                    partial += 1;
                    levels.insert(pixel[3]);
                }
            }
            assert!(transparent > 10000 && partial > 500 && levels.len() > 30);
            let repeat_started = std::time::Instant::now();
            surface
                .draw(&c, &sectors, &path, Some(7), origin, 1.25, false)
                .unwrap();
            let repeat_ms = repeat_started.elapsed().as_secs_f64() * 1000.;
            assert_eq!(
                first,
                bytes(&surface),
                "Decorations and rendering must not flicker between identical frames"
            );
            let output = Path::new(".local/render-review");
            std::fs::create_dir_all(output).unwrap();
            save_png(&surface, &output.join("smooth-transparent.png"));
            save_png(
                &composite(&surface, [248, 248, 248]),
                &output.join("smooth-light.png"),
            );
            save_png(
                &composite(&surface, [24, 20, 16]),
                &output.join("smooth-dark.png"),
            );
            // Render the previous production GDI path at the same geometry for
            // a direct before/after comparison, without any desktop capture.
            let legacy = Surface::new(800, 800).unwrap();
            super::super::render::fill(
                legacy.dc,
                &windows_sys::Win32::Foundation::RECT {
                    left: 0,
                    top: 0,
                    right: 800,
                    bottom: 800,
                },
                0,
            );
            super::super::render::draw(
                legacy.dc,
                &c,
                &sectors,
                &path,
                Some(7),
                origin,
                1.25,
                false,
            );
            GdiFlush();
            let pixels = std::slice::from_raw_parts_mut(legacy.pixels, bytes(&legacy).len());
            for pixel in pixels.chunks_exact_mut(4) {
                pixel[3] = if pixel[..3].iter().any(|channel| *channel != 0) {
                    255
                } else {
                    0
                };
            }
            save_png(
                &composite(&legacy, [248, 248, 248]),
                &output.join("before-light.png"),
            );
            save_png(
                &composite(&legacy, [24, 20, 16]),
                &output.join("before-dark.png"),
            );
            std::fs::write(output.join("render-checks.json"), serde_json::to_vec_pretty(&serde_json::json!({
                "width":800,"height":800,"scale":1.25,"partial_alpha_pixels":partial,
                "alpha_levels":levels.len(),"first_offscreen_frame_ms":first_ms,"repeat_offscreen_frame_ms":repeat_ms,
                "conditions":"Offscreen CPU rasterization including first font use; excludes window composition and input latency",
                "stable_repeat":true,"premultiplied":true
            })).unwrap()).unwrap();
        }
    }
    #[test]
    #[ignore = "offscreen Windows vector rendering; remote CI only"]
    fn exact_arcs_preserve_the_ring_hole_and_scale_with_dpi() {
        unsafe {
            let _engine = Engine::start().unwrap();
            let mut c = Config::default();
            c.root.children.as_mut().unwrap().truncate(1);
            c.layout.gap = 0.;
            let sectors = crate::geometry::visible(&c, &[]);
            for scale in [1., 1.5, 2.] {
                let size = (400. * scale) as i32;
                let origin = Point {
                    x: 200. * scale,
                    y: 200. * scale,
                };
                let mut surface = Surface::new(size, size).unwrap();
                surface
                    .draw(&c, &sectors, &[], None, origin, scale, false)
                    .unwrap();
                let alpha = |radius: f64| {
                    let x = (origin.x + radius * scale) as usize;
                    let y = origin.y as usize;
                    bytes(&surface)[(y * size as usize + x) * 4 + 3]
                };
                assert_eq!(alpha(0.), 255);
                assert_eq!(alpha(40.), 0, "Ring gap must remain transparent");
                assert_eq!(alpha(140.), 255);
                assert_eq!(
                    alpha(147.),
                    0,
                    "Saved radius must not expand with smoothing"
                );
                surface
                    .draw(&c, &sectors, &[], None, origin, scale, true)
                    .unwrap();
                let corner = &bytes(&surface)[..4];
                assert_eq!(
                    corner,
                    &[0, 0, 0, 1],
                    "Custom cursor plane uses minimal nonzero alpha"
                );
                surface
                    .draw(&c, &sectors, &[], None, origin, scale, false)
                    .unwrap();
                assert_eq!(
                    &bytes(&surface)[..4],
                    &[0, 0, 0, 0],
                    "System cursor restores fully transparent gaps"
                );
            }
        }
    }
}
struct Owned<T> {
    pointer: *mut T,
    delete: unsafe fn(*mut T) -> Status,
}
impl<T> Drop for Owned<T> {
    fn drop(&mut self) {
        unsafe {
            (self.delete)(self.pointer);
        }
    }
}
fn owned<T>(
    create: impl FnOnce(*mut *mut T) -> Status,
    delete: unsafe fn(*mut T) -> Status,
) -> Result<Owned<T>, String> {
    let mut pointer = null_mut();
    let result = create(&mut pointer);
    if result != Ok || pointer.is_null() {
        if !pointer.is_null() {
            unsafe {
                delete(pointer);
            }
        }
        return Err(format!(
            "Cannot allocate smooth drawing resource (status {result})"
        ));
    }
    Result::Ok(Owned { pointer, delete })
}
struct Engine(usize);
impl Engine {
    unsafe fn start() -> Result<Self, String> {
        let mut token = 0;
        status(GdiplusStartup(
            &mut token,
            &GdiplusStartupInput {
                GdiplusVersion: 1,
                SuppressExternalCodecs: 1,
                ..zeroed()
            },
            null_mut(),
        ))?;
        Result::Ok(Self(token))
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        unsafe {
            GdiplusShutdown(self.0);
        }
    }
}

struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    pixels: *mut u8,
    width: i32,
    height: i32,
}
impl Surface {
    unsafe fn new(width: i32, height: i32) -> Result<Self, String> {
        if width <= 0 || height <= 0 || width.checked_mul(4).is_none() {
            return Err("Invalid smooth radial surface size".into());
        }
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            return Err("Cannot allocate radial drawing context".into());
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..zeroed()
            },
            ..zeroed()
        };
        let mut pixels = null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut pixels, null_mut(), 0);
        if bitmap.is_null() || pixels.is_null() {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            DeleteDC(dc);
            return Err("Cannot allocate radial pixel surface".into());
        }
        let old = SelectObject(dc, bitmap);
        if old.is_null() || old as isize == -1 {
            DeleteObject(bitmap);
            DeleteDC(dc);
            return Err("Cannot select radial pixel surface".into());
        }
        Result::Ok(Self {
            dc,
            bitmap,
            old,
            pixels: pixels.cast(),
            width,
            height,
        })
    }
    #[cfg(test)]
    unsafe fn draw(
        &mut self,
        c: &Config,
        sectors: &[Sector],
        path: &[u64],
        hover: Option<u64>,
        origin: Point,
        scale: f64,
        cursor_plane: bool,
    ) -> Result<(), String> {
        self.draw_with_icons(
            c,
            sectors,
            path,
            hover,
            origin,
            scale,
            cursor_plane,
            &super::item_icons::Images::new(),
        )
    }
    unsafe fn draw_with_icons(
        &mut self,
        c: &Config,
        sectors: &[Sector],
        path: &[u64],
        hover: Option<u64>,
        origin: Point,
        scale: f64,
        cursor_plane: bool,
        images: &super::item_icons::Images,
    ) -> Result<(), String> {
        let image = owned(
            |out| {
                GdipCreateBitmapFromScan0(
                    self.width,
                    self.height,
                    self.width * 4,
                    PARGB,
                    self.pixels,
                    out,
                )
            },
            |p: *mut GpBitmap| GdipDisposeImage(p.cast()),
        )?;
        let graphics = owned(
            |out| GdipGetImageGraphicsContext(image.pointer.cast(), out),
            |p| GdipDeleteGraphics(p),
        )?;
        status(GdipSetPageUnit(graphics.pointer, UnitPixel))?;
        status(GdipSetSmoothingMode(
            graphics.pointer,
            SmoothingModeAntiAlias8x8,
        ))?;
        status(GdipSetPixelOffsetMode(
            graphics.pointer,
            PixelOffsetModeHalf,
        ))?;
        status(GdipSetInterpolationMode(
            graphics.pointer,
            InterpolationModeHighQualityBicubic,
        ))?;
        status(GdipSetTextRenderingHint(
            graphics.pointer,
            TextRenderingHintAntiAliasGridFit,
        ))?;
        // Alpha 1 retains native cursor ownership over gaps while the radial is
        // open. The 1/255 black backing is only used for the custom cursor;
        // it does not change radial hit geometry or activate the overlay.
        status(GdipGraphicsClear(
            graphics.pointer,
            u32::from(cursor_plane) << 24,
        ))?;
        let mut canvas = Canvas::new(graphics.pointer)?;
        canvas.scene(c, sectors, path, hover, origin, scale, images)?;
        status(GdipFlush(graphics.pointer, FlushIntentionSync))
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

#[derive(Default)]
pub(super) struct Renderer {
    // Field order releases the surface before the graphics engine.
    surface: Option<Surface>,
    engine: Option<Engine>,
    icons: Option<super::item_icons::Cache>,
}
impl Renderer {
    pub unsafe fn present(
        &mut self,
        hwnd: HWND,
        c: &Config,
        sectors: &[Sector],
        path: &[u64],
        hover: Option<u64>,
        origin: Point,
        scale: f64,
        cursor_plane: bool,
        root: &std::path::Path,
    ) -> Result<(), String> {
        if self.engine.is_none() {
            self.engine = Some(Engine::start()?);
        }
        let mut rect = zeroed();
        GetClientRect(hwnd, &mut rect);
        if !self
            .surface
            .as_ref()
            .is_some_and(|s| s.width == rect.right && s.height == rect.bottom)
        {
            self.surface = Some(Surface::new(rect.right, rect.bottom)?);
        }
        let cache = self
            .icons
            .get_or_insert_with(|| super::item_icons::Cache::new(hwnd, root.to_path_buf()));
        let images = cache.collect(
            std::iter::once(&c.root).chain(
                sectors
                    .iter()
                    .filter(|s| {
                        (s.inner + s.outer) * 0.5 * s.sweep * scale >= 18.
                            && (s.outer - s.inner) * scale >= 18.
                    })
                    .filter_map(|s| c.find(s.id)),
            ),
        );
        let surface = self.surface.as_mut().unwrap();
        surface.draw_with_icons(
            c,
            sectors,
            path,
            hover,
            origin,
            scale,
            cursor_plane,
            &images,
        )?;
        let size = SIZE {
            cx: surface.width,
            cy: surface.height,
        };
        let source = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        if UpdateLayeredWindow(
            hwnd,
            null_mut(),
            null(),
            &size,
            surface.dc,
            &source,
            0,
            &blend,
            ULW_ALPHA,
        ) == 0
        {
            return Err(format!(
                "Cannot present smooth radial: {}",
                std::io::Error::last_os_error()
            ));
        }
        Result::Ok(())
    }
    pub fn release_frame(&mut self) {
        self.surface = None;
    }
    pub fn icons_ready(&self) {
        if let Some(cache) = &self.icons {
            cache.acknowledge();
        }
    }
}

struct Canvas {
    graphics: *mut GpGraphics,
    fonts: HashMap<u32, Owned<GpFont>>,
    family: Owned<GpFontFamily>,
    format: Owned<GpStringFormat>,
    brushes: HashMap<u32, Owned<GpSolidFill>>,
    pens: HashMap<(u32, u32), Owned<GpPen>>,
}
fn color(r: u8, g: u8, b: u8, alpha: u8) -> u32 {
    (alpha as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32
}
impl Canvas {
    unsafe fn image(
        &mut self,
        p: Point,
        image: &super::item_icons::Pixels,
        size: f64,
    ) -> Result<(), String> {
        let bitmap = owned(
            |out| {
                GdipCreateBitmapFromScan0(64, 64, 256, PARGB, image.bytes.as_ptr().cast_mut(), out)
            },
            |p: *mut GpBitmap| GdipDisposeImage(p.cast()),
        )?;
        status(GdipDrawImageRectI(
            self.graphics,
            bitmap.pointer.cast(),
            (p.x - size / 2.).round() as i32,
            (p.y - size / 2.).round() as i32,
            size.round().max(1.) as i32,
            size.round().max(1.) as i32,
        ))
    }
    unsafe fn new(graphics: *mut GpGraphics) -> Result<Self, String> {
        let family = owned(
            |out| GdipCreateFontFamilyFromName(wide("Segoe UI").as_ptr(), null_mut(), out),
            |p| GdipDeleteFontFamily(p),
        )?;
        let format = owned(
            |out| GdipStringFormatGetGenericDefault(out),
            |p| GdipDeleteStringFormat(p),
        )?;
        status(GdipSetStringFormatAlign(
            format.pointer,
            StringAlignmentCenter,
        ))?;
        status(GdipSetStringFormatFlags(
            format.pointer,
            StringFormatFlagsNoWrap,
        ))?;
        status(GdipSetStringFormatTrimming(
            format.pointer,
            StringTrimmingEllipsisCharacter,
        ))?;
        Result::Ok(Self {
            graphics,
            family,
            format,
            brushes: HashMap::new(),
            pens: HashMap::new(),
            fonts: HashMap::new(),
        })
    }
    unsafe fn brush(&mut self, value: u32) -> Result<*mut GpBrush, String> {
        if !self.brushes.contains_key(&value) {
            self.brushes.insert(
                value,
                owned(
                    |out| GdipCreateSolidFill(value, out),
                    |p| GdipDeleteBrush(p.cast()),
                )?,
            );
        }
        Result::Ok(self.brushes[&value].pointer.cast())
    }
    unsafe fn pen(&mut self, value: u32, width: f32) -> Result<*mut GpPen, String> {
        let key = (value, width.to_bits());
        if !self.pens.contains_key(&key) {
            let pen = owned(
                |out| GdipCreatePen1(value, width, UnitPixel, out),
                |p| GdipDeletePen(p),
            )?;
            status(GdipSetPenLineJoin(pen.pointer, LineJoinRound))?;
            self.pens.insert(key, pen);
        }
        Result::Ok(self.pens[&key].pointer)
    }
    unsafe fn text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        width: f32,
        size: f32,
        value: u32,
    ) -> Result<(), String> {
        let key = size.to_bits();
        if !self.fonts.contains_key(&key) {
            self.fonts.insert(
                key,
                owned(
                    |out| {
                        GdipCreateFont(self.family.pointer, size, FontStyleRegular, UnitPixel, out)
                    },
                    |p| GdipDeleteFont(p),
                )?,
            );
        }
        let font = self.fonts[&key].pointer;
        let brush = self.brush(value)?;
        let rect = RectF {
            X: x,
            Y: y,
            Width: width,
            Height: size * 1.8,
        };
        let text = wide(text);
        status(GdipDrawString(
            self.graphics,
            text.as_ptr(),
            text.len() as i32 - 1,
            font,
            &rect,
            self.format.pointer,
            brush,
        ))
    }
    unsafe fn circle(&mut self, p: Point, radius: f64, value: u32) -> Result<(), String> {
        let brush = self.brush(value)?;
        status(GdipFillEllipse(
            self.graphics,
            brush,
            (p.x - radius) as f32,
            (p.y - radius) as f32,
            (radius * 2.) as f32,
            (radius * 2.) as f32,
        ))
    }
    unsafe fn sparkle(
        &mut self,
        p: Point,
        radius: f64,
        value: u32,
        filled: bool,
        scale: f64,
    ) -> Result<(), String> {
        let shape = owned(
            |out| GdipCreatePath(FillModeAlternate, out),
            |p| GdipDeletePath(p),
        )?;
        let points: Vec<_> = (0..8)
            .map(|i| {
                let q = p.polar(
                    radius * if i % 2 == 0 { 1. } else { 0.25 },
                    -std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::FRAC_PI_4,
                );
                PointF {
                    X: q.x as f32,
                    Y: q.y as f32,
                }
            })
            .collect();
        status(GdipAddPathPolygon(
            shape.pointer,
            points.as_ptr(),
            points.len() as i32,
        ))?;
        if filled {
            let brush = self.brush(value)?;
            status(GdipFillPath(self.graphics, brush, shape.pointer))
        } else {
            let pen = self.pen(value, scale.max(0.7) as f32)?;
            status(GdipDrawPath(self.graphics, pen, shape.pointer))
        }
    }
    unsafe fn constellation(&mut self, p: Point, scale: f64) -> Result<(), String> {
        for (x, y, radius) in super::icons::CLUSTER {
            self.sparkle(
                Point {
                    x: p.x + x * scale,
                    y: p.y + y * scale,
                },
                radius * scale,
                color(195, 175, 255, 255),
                true,
                scale,
            )?;
        }
        Result::Ok(())
    }
    unsafe fn sector_path(
        &self,
        sector: &Sector,
        origin: Point,
        scale: f64,
    ) -> Result<Owned<GpPath>, String> {
        let shape = owned(
            |out| GdipCreatePath(FillModeAlternate, out),
            |p| GdipDeletePath(p),
        )?;
        let center = Point {
            x: origin.x + sector.center.x * scale,
            y: origin.y + sector.center.y * scale,
        };
        let full = sector.sweep >= std::f64::consts::TAU - 0.000001;
        for (radius, from, sweep) in [
            (sector.outer * scale, sector.from, sector.sweep),
            (
                sector.inner * scale,
                sector.from + sector.sweep,
                -sector.sweep,
            ),
        ] {
            if radius <= 0. {
                continue;
            }
            if full {
                status(GdipAddPathEllipse(
                    shape.pointer,
                    (center.x - radius) as f32,
                    (center.y - radius) as f32,
                    (radius * 2.) as f32,
                    (radius * 2.) as f32,
                ))?;
            } else {
                status(GdipAddPathArc(
                    shape.pointer,
                    (center.x - radius) as f32,
                    (center.y - radius) as f32,
                    (radius * 2.) as f32,
                    (radius * 2.) as f32,
                    from.to_degrees() as f32,
                    sweep.to_degrees() as f32,
                ))?;
            }
        }
        status(GdipClosePathFigure(shape.pointer))?;
        Result::Ok(shape)
    }
    unsafe fn scene(
        &mut self,
        c: &Config,
        sectors: &[Sector],
        path: &[u64],
        hover: Option<u64>,
        origin: Point,
        scale: f64,
        images: &super::item_icons::Images,
    ) -> Result<(), String> {
        let map = |p: Point| Point {
            x: origin.x + p.x * scale,
            y: origin.y + p.y * scale,
        };
        let mut parents = HashMap::new();
        let mut nodes = HashMap::new();
        fn index<'a>(
            node: &'a crate::config::Node,
            nodes: &mut HashMap<u64, &'a crate::config::Node>,
            parents: &mut HashMap<u64, u64>,
        ) {
            nodes.insert(node.id, node);
            if let Some(children) = &node.children {
                for child in children {
                    parents.insert(child.id, node.id);
                    index(child, nodes, parents);
                }
            }
        }
        index(&c.root, &mut nodes, &mut parents);
        let fills = [
            color(28, 43, 59, 255),
            color(43, 38, 67, 255),
            color(40, 100, 102, 255),
            color(89, 69, 131, 255),
        ];
        let edges = [
            color(16, 28, 48, 255),
            color(26, 27, 51, 255),
            color(23, 65, 85, 255),
            color(49, 43, 94, 255),
        ];
        for sector in sectors {
            if sector.sweep <= 0. {
                continue;
            }
            let Some(node) = nodes.get(&sector.id) else {
                continue;
            };
            let folder = node.children.is_some();
            let active = hover == Some(sector.id) || path.contains(&sector.id);
            let palette = usize::from(folder) + 2 * usize::from(active);
            let shape = self.sector_path(sector, origin, scale)?;
            let halo = self.pen(
                (edges[palette] & 0x00ffffff) | 0x46000000,
                (3. * scale) as f32,
            )?;
            status(GdipDrawPath(self.graphics, halo, shape.pointer))?;
            let fill = self.brush(fills[palette])?;
            status(GdipFillPath(self.graphics, fill, shape.pointer))?;
            let edge = self.pen(edges[palette], (3. * scale) as f32)?;
            status(GdipDrawPath(self.graphics, edge, shape.pointer))?;
            // A small deterministic field: never animate or move it on hover.
            let area = 0.5 * (sector.outer.powi(2) - sector.inner.powi(2)) * sector.sweep;
            let count = (area / 4200.).floor().clamp(0., 7.) as usize;
            let mut seed = sector.id.wrapping_mul(0x9e3779b97f4a7c15);
            let mut drawn = 0;
            for _ in 0..count * 5 {
                let unit = |seed: &mut u64| {
                    *seed = seed
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    (*seed >> 40) as f64 / (1u64 << 24) as f64
                };
                let angle = sector.from + sector.sweep * (0.10 + 0.80 * unit(&mut seed));
                let radius =
                    sector.inner + (sector.outer - sector.inner) * (0.15 + 0.70 * unit(&mut seed));
                let point = sector.center.polar(radius, angle);
                let dot_size = 0.9 + 0.75 * unit(&mut seed);
                let alpha = [100, 150, 200][(unit(&mut seed) * 3.) as usize % 3];
                if (point.x - sector.dot.x).abs() < 65. && (point.y - sector.dot.y).abs() < 32. {
                    continue;
                }
                self.circle(map(point), dot_size * scale, color(255, 255, 255, alpha))?;
                drawn += 1;
                if drawn >= count {
                    break;
                }
            }
            let arc = (sector.inner + sector.outer) * 0.5 * sector.sweep * scale;
            if arc < 18. || (sector.outer - sector.inner) * scale < 18. {
                continue;
            }
            let point = map(sector.dot);
            let symbol = Point {
                x: point.x,
                y: point.y - 15. * scale,
            };
            if let Some(image) = images.get(&node.id) {
                self.image(symbol, image, 24. * scale)?;
            } else if folder {
                self.constellation(symbol, scale)?;
            } else {
                self.sparkle(symbol, 7.5 * scale, color(142, 226, 205, 255), true, scale)?;
            }
            let width = arc.clamp(12. * scale, 125. * scale);
            self.text(
                &node.name,
                (point.x - width / 2.) as f32,
                (point.y + 3. * scale) as f32,
                width as f32,
                (12. * scale).max(7.) as f32,
                color(233, 237, 250, 255),
            )?;
        }
        let lit: Vec<_> = sectors
            .iter()
            .filter(|s| path.contains(&s.id) || hover == Some(s.id))
            .collect();
        let by_id: HashMap<_, _> = lit.iter().map(|s| (s.id, *s)).collect();
        for sector in &lit {
            if let Some(parent) = parents.get(&sector.id).and_then(|id| by_id.get(id)) {
                let (a, b) = (map(parent.dot), map(sector.dot));
                let pen = self.pen(color(172, 210, 247, 210), (1.25 * scale) as f32)?;
                status(GdipDrawLine(
                    self.graphics,
                    pen,
                    a.x as f32,
                    a.y as f32,
                    b.x as f32,
                    b.y as f32,
                ))?;
            }
        }
        for sector in &lit {
            let point = map(sector.dot);
            self.circle(point, 6. * scale, color(76, 98, 142, 150))?;
            self.circle(point, 3. * scale, color(235, 250, 255, 255))?;
        }
        let current = path.last().and_then(|id| c.find(*id)).unwrap_or(&c.root);
        if current
            .children
            .as_ref()
            .is_some_and(|children| children.is_empty())
        {
            self.text(
                &format!("{} is empty · Backspace or center to return", current.name),
                (origin.x - 230. * scale) as f32,
                (origin.y + (c.layout.radius + 10.) * scale) as f32,
                (460. * scale) as f32,
                (13. * scale) as f32,
                color(205, 215, 235, 255),
            )?;
        }
        let radius = (c.layout.radius * 0.27 * scale).max(3.);
        self.circle(origin, radius, color(18, 25, 39, 255))?;
        let center_symbol = Point {
            x: origin.x,
            y: origin.y - 7. * scale,
        };
        if let Some(image) = images.get(&c.root.id) {
            self.image(center_symbol, image, 24. * scale)?;
        } else {
            self.constellation(center_symbol, scale)?;
        }
        if radius > 25. * scale {
            self.text(
                "ROOT",
                (origin.x - 28. * scale) as f32,
                (origin.y + 7. * scale) as f32,
                (56. * scale) as f32,
                (9. * scale) as f32,
                color(156, 175, 207, 255),
            )?;
        }
        Result::Ok(())
    }
}
