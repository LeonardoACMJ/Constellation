//! Small, deterministic icon decorations. Input/output are premultiplied BGRA.
//! Only the asynchronous icon loader calls this; paints reuse the cached pixels.
use crate::config::IconStyle;
type Point = (f64, f64);
const SCALE: usize = 4;
struct Canvas {
    side: usize,
    pixels: Vec<u8>,
}
impl Canvas {
    fn paint(&mut self, x: usize, y: usize, color: [u8; 4]) {
        let i = (y * self.side + x) * 4;
        for (dst, src) in self.pixels[i..i + 4].iter_mut().zip(color) {
            *dst =
                (src as u32 + (*dst as u32 * (255 - color[3]) as u32 + 127) / 255).min(255) as u8;
        }
    }
    fn shape(&mut self, color: u32, inside: impl Fn(Point) -> bool) {
        let rgba = [
            (color & 255) as u8,
            ((color >> 8) & 255) as u8,
            ((color >> 16) & 255) as u8,
            255,
        ];
        for y in 0..self.side {
            for x in 0..self.side {
                let p = (
                    (x as f64 + 0.5) * 256. / self.side as f64,
                    (y as f64 + 0.5) * 256. / self.side as f64,
                );
                if inside(p) {
                    self.paint(x, y, rgba);
                }
            }
        }
    }
    fn polygon(&mut self, pts: &[Point], color: u32) {
        self.shape(color, |(x, y)| {
            let mut inside = false;
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
                    inside = !inside;
                }
            }
            inside
        });
    }
    fn line(&mut self, pts: &[Point], color: u32, width: f64) {
        self.shape(color, |p| {
            pts.windows(2).any(|s| {
                let (a, b) = (s[0], s[1]);
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy).max(0.001))
                    .clamp(0., 1.);
                (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy) <= width / 2.
            })
        });
    }
    fn rounded(&mut self, rect: [f64; 4], r: f64, color: u32) {
        self.shape(color, |(x, y)| {
            let [l, t, right, bottom] = rect;
            if x < l || x > right || y < t || y > bottom {
                return false;
            }
            let cx = x.clamp(l + r, right - r);
            let cy = y.clamp(t + r, bottom - r);
            (x - cx).hypot(y - cy) <= r
        });
    }
    fn star(&mut self, x: f64, y: f64, r: f64, fill: u32, outline: u32, width: f64) {
        let mut pts: Vec<_> = (0..8)
            .map(|i| {
                let a = -std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::FRAC_PI_4;
                let r = if i % 2 == 0 { r } else { r * 0.28 };
                (x + a.cos() * r, y + a.sin() * r)
            })
            .collect();
        self.polygon(&pts, fill);
        pts.push(pts[0]);
        self.line(&pts, outline, width);
    }
    fn image(&mut self, bytes: &[u8], size: usize, left: f64, top: f64, extent: f64) {
        for y in 0..self.side {
            for x in 0..self.side {
                let px = (x as f64 + 0.5) * 256. / self.side as f64;
                let py = (y as f64 + 0.5) * 256. / self.side as f64;
                if px < left || py < top || px >= left + extent || py >= top + extent {
                    continue;
                }
                let sx = ((px - left) / extent * size as f64 - 0.5).clamp(0., (size - 1) as f64);
                let sy = ((py - top) / extent * size as f64 - 0.5).clamp(0., (size - 1) as f64);
                let (ix, iy) = (sx.floor() as usize, sy.floor() as usize);
                let (tx, ty) = (sx - ix as f64, sy - iy as f64);
                let mut color = [0; 4];
                for (c, value) in color.iter_mut().enumerate() {
                    let at = |xx, yy| bytes[(yy * size + xx) * 4 + c] as f64;
                    let a = at(ix, iy) * (1. - tx) + at((ix + 1).min(size - 1), iy) * tx;
                    let b = at(ix, (iy + 1).min(size - 1)) * (1. - tx)
                        + at((ix + 1).min(size - 1), (iy + 1).min(size - 1)) * tx;
                    *value = (a * (1. - ty) + b * ty).round() as u8;
                }
                self.paint(x, y, color);
            }
        }
    }
}
/// Preserve alpha and bound temporary memory even if a future caller supplies
/// malformed dimensions. Original is byte-identical; failed sources stay absent.
pub fn compose(bytes: &[u8], size: usize, style: IconStyle) -> Option<Vec<u8>> {
    if size == 0 || size > 256 || bytes.len() != size * size * 4 {
        return None;
    }
    if style.is_original() {
        return Some(bytes.to_vec());
    }
    let side = size * SCALE;
    let mut c = Canvas {
        side,
        pixels: vec![0; side * side * 4],
    };
    match style {
        IconStyle::Original => unreachable!(),
        IconStyle::Folder => {
            c.rounded([16., 41., 116., 92.], 12., 0x77531e);
            c.rounded([19., 44., 113., 92.], 9., 0xc78f2c);
            c.rounded([16., 65., 229., 219.], 14., 0x77531e);
            c.rounded([19., 68., 226., 216.], 11., 0xd99c32);
            c.rounded([26., 78., 220., 121.], 5., 0xffe5a6);
            let pts = [(17., 102.), (235., 102.), (217., 221.), (33., 221.)];
            c.polygon(&pts, 0xf6c45b);
            c.line(&[pts[0], pts[1], pts[2], pts[3], pts[0]], 0x9f7125, 3.);
            c.image(bytes, size, 109., 106., 142.);
        }
        IconStyle::Constellation => {
            c.image(bytes, size, 12., 52., 188.);
            c.line(&[(164., 52.), (207., 28.), (232., 85.)], 0xadbde8, 3.);
            c.star(164., 52., 20., 0xd8cdff, 0x18223b, 2.);
            c.star(207., 28., 24., 0xd8cdff, 0x18223b, 2.);
            c.star(232., 85., 16., 0xd8cdff, 0x18223b, 2.);
        }
        IconStyle::StarFrame => {
            c.star(128., 128., 121., 0x252940, 0xb7a6e8, 4.);
            c.image(bytes, size, 54., 54., 148.);
        }
    }
    let mut out = vec![0; bytes.len()];
    for y in 0..size {
        for x in 0..size {
            for channel in 0..4 {
                let mut total = 0u32;
                for dy in 0..SCALE {
                    for dx in 0..SCALE {
                        total += c.pixels[((y * SCALE + dy) * side + x * SCALE + dx) * 4 + channel]
                            as u32;
                    }
                }
                out[(y * size + x) * 4 + channel] = ((total + 8) / 16) as u8;
            }
        }
    }
    Some(out)
}
