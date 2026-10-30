//! Small portable palette; default configurations keep their original shape.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u32);
impl Color {
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim().strip_prefix('#').unwrap_or(text.trim());
        if text.len() != 6 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Use a six-digit color, for example #8EE2CD".into());
        }
        Ok(Self(
            u32::from_str_radix(text, 16).map_err(|e| e.to_string())?,
        ))
    }
    pub fn hex(self) -> String {
        format!("#{:06X}", self.0)
    }
    pub fn argb(self, alpha: u8) -> u32 {
        ((alpha as u32) << 24) | self.0
    }
    pub fn gdi(self) -> u32 {
        ((self.0 & 255) << 16) | (self.0 & 0xff00) | (self.0 >> 16)
    }
}
impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex())
    }
}
impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Palette {
    pub background: Color,
    pub input: Color,
    pub text: Color,
    /// Optional Star-label override; Constellations and editor text retain `text`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub star_text: Option<Color>,
    pub muted: Color,
    pub star: Color,
    pub constellation: Color,
    pub star_highlight: Color,
    pub constellation_highlight: Color,
    pub outline: Color,
    /// Per-state outline colors. `None` derives a blue-shifted shade from the
    /// base outline and that sector's fill; each can be overridden separately.
    #[serde(default = "auto_outline", skip_serializing_if = "Option::is_none")]
    pub star_outline: Option<Color>,
    #[serde(default = "auto_outline", skip_serializing_if = "Option::is_none")]
    pub constellation_outline: Option<Color>,
    #[serde(default = "auto_outline", skip_serializing_if = "Option::is_none")]
    pub star_highlight_outline: Option<Color>,
    #[serde(default = "auto_outline", skip_serializing_if = "Option::is_none")]
    pub constellation_highlight_outline: Option<Color>,
    pub line: Color,
    pub symbol: Color,
    pub cluster: Color,
    pub hub: Color,
}
fn auto_outline() -> Option<Color> {
    None
}
impl Default for Palette {
    fn default() -> Self {
        Self {
            background: Color(0x0f1624),
            input: Color(0x172132),
            text: Color(0xe9edfa),
            star_text: None,
            muted: Color(0x94a4be),
            star: Color(0x1c2b3b),
            constellation: Color(0x2b2643),
            star_highlight: Color(0x286466),
            constellation_highlight: Color(0x594583),
            outline: Color(0x101c30),
            star_outline: Some(Color(0x101c30)),
            constellation_outline: Some(Color(0x1a1b33)),
            star_highlight_outline: Some(Color(0x174155)),
            constellation_highlight_outline: Some(Color(0x312b5e)),
            line: Color(0xacd2f7),
            symbol: Color(0x8ee2cd),
            cluster: Color(0xc3afff),
            hub: Color(0x121927),
        }
    }
}
impl Palette {
    pub const LABELS: [&'static str; 13] = [
        "Background",
        "Fields / panels",
        "Text",
        "Secondary text",
        "Star sector",
        "Constellation sector",
        "Star highlight",
        "Constellation highlight",
        "Outline",
        "Connecting line",
        "Star symbol",
        "Constellation symbol",
        "Center hub",
    ];
    pub const OUTLINE_LABELS: [&'static str; 4] = [
        "Star outline",
        "Constellation outline",
        "Highlighted star outline",
        "Highlighted constellation outline",
    ];
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn colors(self) -> [Color; 13] {
        [
            self.background,
            self.input,
            self.text,
            self.muted,
            self.star,
            self.constellation,
            self.star_highlight,
            self.constellation_highlight,
            self.outline,
            self.line,
            self.symbol,
            self.cluster,
            self.hub,
        ]
    }
    pub fn set(&mut self, index: usize, color: Color) {
        let mut colors = self.colors();
        if let Some(value) = colors.get_mut(index) {
            *value = color;
            let outlines = self.outline_overrides();
            let star_text = self.star_text;
            *self = Self::from_colors(colors);
            self.set_outline_overrides(outlines);
            self.star_text = star_text;
        }
    }
    pub fn outline_overrides(self) -> [Option<Color>; 4] {
        [
            self.star_outline,
            self.constellation_outline,
            self.star_highlight_outline,
            self.constellation_highlight_outline,
        ]
    }
    pub fn label_color(self, constellation: bool) -> Color {
        if constellation {
            self.text
        } else {
            self.star_text.unwrap_or(self.text)
        }
    }
    pub fn set_outline(&mut self, index: usize, color: Option<Color>) {
        match index {
            0 => self.star_outline = color,
            1 => self.constellation_outline = color,
            2 => self.star_highlight_outline = color,
            3 => self.constellation_highlight_outline = color,
            _ => {}
        }
    }
    fn set_outline_overrides(&mut self, colors: [Option<Color>; 4]) {
        self.star_outline = colors[0];
        self.constellation_outline = colors[1];
        self.star_highlight_outline = colors[2];
        self.constellation_highlight_outline = colors[3];
    }
    /// Inactive Star, inactive Constellation, highlighted Star, highlighted
    /// Constellation. Missing overrides stay darker than their fills and shift
    /// gently toward blue so hand-made palettes need only choose one outline.
    pub fn outline_colors(self) -> [Color; 4] {
        let automatic = [
            derive_outline(self.outline, self.star, Color(0x24496c)),
            derive_outline(self.outline, self.constellation, Color(0x3b3a67)),
            derive_outline(self.outline, self.star_highlight, Color(0x315f7e)),
            derive_outline(self.outline, self.constellation_highlight, Color(0x514a7d)),
        ];
        let overrides = self.outline_overrides();
        std::array::from_fn(|i| overrides[i].unwrap_or(automatic[i]))
    }
    fn from_colors(c: [Color; 13]) -> Self {
        Self {
            background: c[0],
            input: c[1],
            text: c[2],
            star_text: None,
            muted: c[3],
            star: c[4],
            constellation: c[5],
            star_highlight: c[6],
            constellation_highlight: c[7],
            outline: c[8],
            star_outline: None,
            constellation_outline: None,
            star_highlight_outline: None,
            constellation_highlight_outline: None,
            line: c[9],
            symbol: c[10],
            cluster: c[11],
            hub: c[12],
        }
    }
    pub fn preset(index: usize) -> Self {
        let mut p = Self::default();
        match index {
            1 => {
                p.background = Color(0x151b19);
                p.input = Color(0x202c25);
                p.star = Color(0x263d32);
                p.constellation = Color(0x38472c);
                p.star_highlight = Color(0x427862);
                p.constellation_highlight = Color(0x65733e);
                p.outline = Color(0x14251e);
                p.set_outline_overrides([
                    Some(Color(0x162d25)),
                    Some(Color(0x26321f)),
                    Some(Color(0x386d56)),
                    Some(Color(0x596535)),
                ]);
                p.symbol = Color(0xb0e6be);
                p.cluster = Color(0xe0df9b);
                p.hub = Color(0x18231c);
            }
            2 => {
                p.background = Color(0x251922);
                p.input = Color(0x35232d);
                p.star = Color(0x4a2d39);
                p.constellation = Color(0x49334f);
                p.star_highlight = Color(0x986078);
                p.constellation_highlight = Color(0x805c9c);
                p.outline = Color(0x291b2c);
                p.set_outline_overrides([
                    Some(Color(0x2b1c2c)),
                    Some(Color(0x35213b)),
                    Some(Color(0x754b62)),
                    Some(Color(0x694b80)),
                ]);
                p.symbol = Color(0xffc4d4);
                p.cluster = Color(0xdfc1ff);
                p.hub = Color(0x2d1e29);
            }
            3 => {
                p.background = Color(0xf1f3f8);
                p.input = Color(0xffffff);
                p.text = Color(0x182237);
                p.muted = Color(0x4b5971);
                p.star = Color(0xd8e6ed);
                p.constellation = Color(0xe4dcf0);
                p.star_highlight = Color(0x90c9c3);
                p.constellation_highlight = Color(0xbba2da);
                p.outline = Color(0x6a7992);
                p.set_outline_overrides([
                    Some(Color(0x596e84)),
                    Some(Color(0x6b6384)),
                    Some(Color(0x528c87)),
                    Some(Color(0x7f69a0)),
                ]);
                p.line = Color(0x4c5b85);
                p.symbol = Color(0x146a60);
                p.cluster = Color(0x60418a);
                p.hub = Color(0xebedf4);
            }
            _ => {}
        };
        p
    }
}

fn derive_outline(outline: Color, fill: Color, blue_tint: Color) -> Color {
    let shifted = mix(mix(outline, blue_tint, 18), fill, 8);
    let brightness = |color: Color| {
        let red = ((color.0 >> 16) & 255) as f64;
        let green = ((color.0 >> 8) & 255) as f64;
        let blue = (color.0 & 255) as f64;
        red * 0.299 + green * 0.587 + blue * 0.114
    };
    let source = brightness(shifted);
    let target = source.min(brightness(fill) * 0.52);
    if source <= 0.0 {
        return shifted;
    }
    let scale = target / source;
    let red = (((shifted.0 >> 16) & 255) as f64 * scale).round() as u32;
    let green = (((shifted.0 >> 8) & 255) as f64 * scale).round() as u32;
    let blue = ((shifted.0 & 255) as f64 * scale).round() as u32;
    Color((red << 16) | (green << 8) | blue)
}

fn mix(first: Color, second: Color, weight: u32) -> Color {
    let channel = |shift: u32| {
        ((((first.0 >> shift) & 255) * (100 - weight) + ((second.0 >> shift) & 255) * weight + 50)
            / 100)
            << shift
    };
    Color(channel(16) | channel(8) | channel(0))
}
