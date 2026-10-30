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
    pub muted: Color,
    pub star: Color,
    pub constellation: Color,
    pub star_highlight: Color,
    pub constellation_highlight: Color,
    pub outline: Color,
    pub line: Color,
    pub symbol: Color,
    pub cluster: Color,
    pub hub: Color,
}
impl Default for Palette {
    fn default() -> Self {
        Self {
            background: Color(0x0f1624),
            input: Color(0x172132),
            text: Color(0xe9edfa),
            muted: Color(0x94a4be),
            star: Color(0x1c2b3b),
            constellation: Color(0x2b2643),
            star_highlight: Color(0x286466),
            constellation_highlight: Color(0x594583),
            outline: Color(0x101c30),
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
            *self = Self::from_colors(colors);
        }
    }
    fn from_colors(c: [Color; 13]) -> Self {
        Self {
            background: c[0],
            input: c[1],
            text: c[2],
            muted: c[3],
            star: c[4],
            constellation: c[5],
            star_highlight: c[6],
            constellation_highlight: c[7],
            outline: c[8],
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
