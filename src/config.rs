use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Short, single-line usage advice; count Unicode scalar values, not bytes.
pub const NOTE_MAX_CHARS: usize = 140;
pub fn note_break(c: char) -> bool {
    c.is_control() || matches!(c, '\u{2028}' | '\u{2029}')
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "ConfigInput", into = "ConfigOutput")]
pub struct Config {
    pub version: u32,
    /// Independent trees. A shortcut profile opens exactly one of them, so node
    /// IDs stay unique across all of them and `find` never needs a tree argument.
    pub universes: Vec<Node>,
    pub profiles: Vec<Profile>,
    pub layout: Layout,
    pub sound: Sound,
}

/// Reads both shapes: `root` (every version through 0.1.25) and `universes`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigInput {
    version: u32,
    #[serde(default)]
    root: Option<Node>,
    #[serde(default)]
    universes: Option<Vec<Node>>,
    profiles: Vec<Profile>,
    #[serde(default)]
    layout: Layout,
    #[serde(default)]
    sound: Sound,
}
impl From<ConfigInput> for Config {
    fn from(c: ConfigInput) -> Self {
        // `universes` wins if a hand-edited file carries both; an empty list is
        // left for validate() to reject with a readable message.
        let universes = c
            .universes
            .or_else(|| c.root.map(|r| vec![r]))
            .unwrap_or_default();
        Self {
            version: c.version,
            universes,
            profiles: c.profiles,
            layout: c.layout,
            sound: c.sound,
        }
    }
}

/// Writes a single-universe configuration back in the original `root` shape, so a
/// file only changes form once a second universe actually exists.
#[derive(Serialize)]
struct ConfigOutput {
    version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    root: Option<Node>,
    #[serde(skip_serializing_if = "Option::is_none")]
    universes: Option<Vec<Node>>,
    profiles: Vec<Profile>,
    layout: Layout,
    sound: Sound,
}
impl From<Config> for ConfigOutput {
    fn from(c: Config) -> Self {
        let mut universes = c.universes;
        let single = universes.len() == 1;
        Self {
            version: c.version,
            root: single.then(|| universes.remove(0)),
            universes: (!single).then_some(universes),
            profiles: c.profiles,
            layout: c.layout,
            sound: c.sound,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    pub id: u64,
    pub name: String,
    /// A node is either a constellation (Some children, including empty), or a Star.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<Node>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
    #[serde(default, skip_serializing_if = "ItemIcon::is_auto")]
    pub icon: ItemIcon,
    /// Shortcut profiles that show this item. Empty means every profile, so an
    /// item is shared unless it is deliberately restricted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visible_in: Vec<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum ItemIcon {
    #[default]
    Automatic,
    Symbol,
    Custom {
        path: String,
    },
    Software {
        path: String,
    },
}
impl ItemIcon {
    pub fn is_auto(&self) -> bool {
        matches!(self, Self::Automatic)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    Program {
        path: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
    },
    Shortcut {
        path: String,
    },
    Url {
        url: String,
    },
    Folder {
        path: String,
    },
    Script {
        path: String,
        #[serde(default)]
        runner: Option<String>,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "ProfileInput")]
pub struct Profile {
    pub id: u64,
    pub name: String,
    pub binding: String,
    pub mode: Mode,
    pub hover: bool,
    pub cursor_end: CursorEnd,
    pub directional: bool,
    pub dot_cursor: bool,
    /// Universe this binding opens. 0 means the first one, so a configuration
    /// that never used universes keeps its original profile shape on disk.
    #[serde(default, skip_serializing_if = "is_first_universe")]
    pub universe: u64,
    /// Open at this node instead of at the universe root. The node must be a
    /// constellation inside `universe`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_id: Option<u64>,
}
fn is_first_universe(id: &u64) -> bool {
    *id == 0
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileInput {
    #[serde(default)]
    id: u64,
    name: String,
    binding: String,
    #[serde(default)]
    mode: Mode,
    #[serde(default = "yes")]
    hover: bool,
    #[serde(default)]
    cursor_end: Option<CursorEnd>,
    #[serde(default)]
    restore_cursor: Option<bool>,
    #[serde(default = "yes")]
    directional: bool,
    #[serde(default = "yes")]
    dot_cursor: bool,
    #[serde(default)]
    universe: u64,
    #[serde(default)]
    root_id: Option<u64>,
}
impl From<ProfileInput> for Profile {
    fn from(p: ProfileInput) -> Self {
        Self {
            id: p.id,
            name: p.name,
            binding: p.binding,
            mode: p.mode,
            hover: p.hover,
            cursor_end: p.cursor_end.unwrap_or(if p.restore_cursor.unwrap_or(true) {
                CursorEnd::Restore
            } else {
                CursorEnd::Leave
            }),
            directional: p.directional,
            dot_cursor: p.dot_cursor,
            universe: p.universe,
            root_id: p.root_id,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorEnd {
    #[default]
    Restore,
    Center,
    Leave,
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Click,
    Hold,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Layout {
    pub radius: f64,
    pub distance: f64,
    pub spread: f64,
    pub gap: f64,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            radius: 144.,
            distance: 0.52,
            spread: 148.,
            gap: 2.,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sound {
    #[serde(skip_serializing_if = "Instrument::is_default")]
    pub instrument: Instrument,
    pub enabled: bool,
    pub volume: f64,
}
impl Sound {
    /// None is the editor's silent choice; keep the existing portable schema.
    pub fn selected_instrument(&self) -> Option<Instrument> {
        self.enabled.then_some(self.instrument)
    }
    pub fn select_instrument(&mut self, instrument: Option<Instrument>) {
        self.enabled = instrument.is_some();
        if let Some(instrument) = instrument {
            self.instrument = instrument;
        }
    }
}
impl Default for Sound {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: 0.18,
            instrument: Instrument::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Instrument {
    #[default]
    Mallet,
    ClassicBell,
    PluckedStrings,
    WindChimes,
}
impl Instrument {
    pub fn is_default(&self) -> bool {
        *self == Self::Mallet
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Binding {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Input,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Input {
    Key(u16),
    Mouse(u16),
}
impl Binding {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut b = Self {
            ctrl: false,
            shift: false,
            alt: false,
            key: Input::Key(0),
        };
        let mut key = None;
        for part in text.split('+').map(|p| p.trim().to_ascii_uppercase()) {
            match part.as_str(){
                "CTRL"|"CONTROL"=>b.ctrl=true,"SHIFT"=>b.shift=true,"ALT"=>b.alt=true,
                "MOUSE1"|"MOUSE2"=>return Err("Use Mouse3 (middle), Mouse4 or Mouse5; primary click buttons are reserved for selection/dismissal".into()),
                "MOUSE3"|"MOUSE4"|"MOUSE5"=>{if key.is_some(){return Err("Only one trigger is allowed".into())}key=Some(Input::Mouse(part.as_bytes()[5] as u16-b'0' as u16));},
                _=>{
                    if key.is_some(){return Err("Only one trigger is allowed".into())}
                    let vk=if part.len()==1&&part.as_bytes()[0].is_ascii_alphanumeric(){part.as_bytes()[0] as u16}
                        else if let Some(n)=part.strip_prefix('F').and_then(|v|v.parse::<u16>().ok()).filter(|n|(1..=24).contains(n)){if n==12{return Err("F12 is reserved by Windows debugging".into())}0x70+n-1}
                        else if part=="SPACE"{0x20}else{return Err(format!("Unsupported binding component: {part}"))};
                    key=Some(Input::Key(vk));
                }
            }
        }
        b.key = key.ok_or("A trigger key is required")?;
        if matches!(b.key, Input::Mouse(_)) && (b.ctrl || b.alt || b.shift) {
            return Err(
                "Use an unmodified mouse button, or map it to a keyboard combination".into(),
            );
        }
        Ok(b)
    }
    pub fn requires(&self, key: Input) -> bool {
        self.key == key
            || matches!(key, Input::Key(0x11 | 0xa2 | 0xa3)) && self.ctrl
            || matches!(key, Input::Key(0x10 | 0xa0 | 0xa1)) && self.shift
            || matches!(key, Input::Key(0x12 | 0xa4 | 0xa5)) && self.alt
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("Unsupported configuration version".into());
        }
        if self.universes.is_empty() {
            return Err("A configuration needs at least one universe".into());
        }
        if self.universes.iter().any(|u| u.children.is_none()) {
            return Err("Every universe root must be a constellation".into());
        }
        let mut ids = HashSet::new();
        fn walk(n: &Node, ids: &mut HashSet<u64>, depth: usize) -> Result<(), String> {
            if depth > 64 {
                return Err("Configuration nesting exceeds parser safety limit (64)".into());
            }
            if !ids.insert(n.id) {
                return Err(format!("Duplicate node ID {}", n.id));
            }
            if n.note.chars().count() > NOTE_MAX_CHARS || n.note.chars().any(note_break) {
                return Err(format!(
                    "Note for {} must be a single line of at most {NOTE_MAX_CHARS} characters",
                    n.name
                ));
            }
            if n.name.trim().is_empty() {
                return Err("Every node needs a name".into());
            }
            if let ItemIcon::Custom { path } | ItemIcon::Software { path } = &n.icon {
                if path.trim().is_empty() || path.contains('\0') {
                    return Err(format!("Invalid icon source for {}", n.name));
                }
            }
            match (&n.children, &n.target) {
                (Some(children), None) => {
                    for c in children {
                        walk(c, ids, depth + 1)?
                    }
                }
                (None, Some(t)) => {
                    let p = match t {
                        Target::Program { path, .. }
                        | Target::Shortcut { path }
                        | Target::Folder { path }
                        | Target::Script { path, .. } => path,
                        Target::Url { url } => url,
                    };
                    if p.trim().is_empty() || p.contains('\0') {
                        return Err(format!("Invalid target for {}", n.name));
                    }
                    if let Target::Url { url } = t {
                        if !url.starts_with("https://") && !url.starts_with("http://") {
                            return Err("URL targets must use http:// or https://".into());
                        }
                    }
                }
                _ => {
                    return Err(format!(
                        "{} must have either children or target, not both",
                        n.name
                    ));
                }
            }
            Ok(())
        }
        // One ID space across every universe: that is what lets find() and the
        // navigation path stay free of a tree argument.
        for universe in &self.universes {
            walk(universe, &mut ids, 0)?;
        }
        let l = self.layout;
        if ![l.radius, l.distance, l.spread, l.gap, self.sound.volume]
            .iter()
            .all(|x| x.is_finite())
            || l.radius <= 0.
            || l.distance < 0.
            || l.spread <= 0.
            || l.spread > 360.
            || l.gap < 0.
        {
            return Err(
                "Layout values must be finite; radius > 0, distance/gap >= 0 and spread in (0,360]"
                    .into(),
            );
        }
        if !(0. ..=1.).contains(&self.sound.volume) {
            return Err("Sound volume must be between 0 and 1".into());
        }
        let mut profile_ids = HashSet::new();
        let mut bindings = HashSet::new();
        for p in &self.profiles {
            let b = Binding::parse(&p.binding)?;
            if !bindings.insert(b) {
                return Err("Duplicate shortcut bindings".into());
            }
            if p.id != 0 && !profile_ids.insert(p.id) {
                return Err("Duplicate profile ID".into());
            }
            if p.name.trim().is_empty() {
                return Err("Every shortcut profile needs a name".into());
            }
            let Some(universe) = self.universe_of(p) else {
                return Err(format!("{} opens a universe that does not exist", p.name));
            };
            // An entry point has to be a constellation of that same universe:
            // a Star has nothing to fan out, and a node borrowed from another
            // universe would open a tree the profile does not own.
            if let Some(entry) = p.root_id {
                match universe.find(entry) {
                    None => {
                        return Err(format!("{} opens a node outside its universe", p.name));
                    }
                    Some(n) if n.children.is_none() => {
                        return Err(format!("{} must open a constellation, not a Star", p.name));
                    }
                    Some(_) => {}
                }
            }
        }
        // A restriction naming a profile that opens a different universe is dead:
        // the item is hidden everywhere and nothing ever shows it. Say so here
        // rather than letting it read as a vanished item.
        for universe in &self.universes {
            let audience: HashSet<u64> = self
                .profiles
                .iter()
                .filter(|p| self.universe_of(p).is_some_and(|u| u.id == universe.id))
                .map(|p| p.id)
                .collect();
            fn restricted(n: &Node, audience: &HashSet<u64>, all: &HashSet<u64>) -> Result<(), String> {
                for id in &n.visible_in {
                    if !all.contains(id) {
                        return Err(format!("{} is restricted to a profile that does not exist", n.name));
                    }
                    if !audience.contains(id) {
                        return Err(format!(
                            "{} is restricted to a profile that opens another universe, so nothing would show it",
                            n.name
                        ));
                    }
                }
                for c in n.children.as_deref().unwrap_or_default() {
                    restricted(c, audience, all)?;
                }
                Ok(())
            }
            restricted(universe, &audience, &profile_ids)?;
        }
        Ok(())
    }
    /// The universe a profile opens. `universe == 0` means the first one, which
    /// keeps every pre-universe configuration working untouched.
    pub fn universe_of(&self, p: &Profile) -> Option<&Node> {
        if p.universe == 0 {
            return self.universes.first();
        }
        self.universes.iter().find(|u| u.id == p.universe)
    }
    /// Where this profile's radial opens: its entry node, or the universe root.
    pub fn entry_of(&self, p: &Profile) -> Option<&Node> {
        let universe = self.universe_of(p)?;
        match p.root_id {
            Some(id) => universe.find(id),
            None => Some(universe),
        }
    }
    /// Highest node ID anywhere. New nodes take the next one so an ID is never
    /// reused while a copy of it still exists in another universe.
    pub fn max_node_id(&self) -> u64 {
        self.universes.iter().map(Node::max_id).max().unwrap_or(0)
    }
    /// Upgrade legacy profiles without IDs. Explicit IDs are never rewritten.
    pub fn assign_profile_ids(&mut self) -> Result<(), String> {
        let mut next = self.profiles.iter().map(|p| p.id).max().unwrap_or(0);
        for p in &mut self.profiles {
            if p.id == 0 {
                next = next.checked_add(1).ok_or("Profile ID space exhausted")?;
                p.id = next;
            }
        }
        Ok(())
    }
    pub fn find(&self, id: u64) -> Option<&Node> {
        self.universes.iter().find_map(|u| u.find(id))
    }
}
impl Node {
    pub fn find(&self, id: u64) -> Option<&Self> {
        if self.id == id {
            return Some(self);
        }
        self.children.as_ref()?.iter().find_map(|c| c.find(id))
    }
    pub fn find_mut(&mut self, id: u64) -> Option<&mut Self> {
        if self.id == id {
            return Some(self);
        }
        self.children
            .as_mut()?
            .iter_mut()
            .find_map(|c| c.find_mut(id))
    }
    pub fn max_id(&self) -> u64 {
        self.children
            .as_ref()
            .map(|cs| cs.iter().map(Self::max_id).max().unwrap_or(0))
            .unwrap_or(0)
            .max(self.id)
    }
}
impl Default for Config {
    fn default() -> Self {
        fn star(id: u64, name: &str, target: Target) -> Node {
            Node {
                note: String::new(),
                icon: Default::default(),
                visible_in: vec![],
                id,
                name: name.into(),
                children: None,
                target: Some(target),
            }
        }
        Self {
            version: 1,
            universes: vec![Node {
                note: String::new(),
                icon: Default::default(),
                visible_in: vec![],
                id: 1,
                name: "Your universe".into(),
                target: None,
                children: Some(vec![
                    Node {
                        note: String::new(),
                        icon: Default::default(),
                        visible_in: vec![],
                        id: 2,
                        name: "Everyday".into(),
                        target: None,
                        children: Some(vec![
                            star(
                                3,
                                "Calculator",
                                Target::Program {
                                    path: "calc.exe".into(),
                                    args: vec![],
                                    cwd: None,
                                },
                            ),
                            star(
                                4,
                                "Notepad",
                                Target::Program {
                                    path: "notepad.exe".into(),
                                    args: vec![],
                                    cwd: None,
                                },
                            ),
                            Node {
                                note: String::new(),
                                icon: Default::default(),
                                visible_in: vec![],
                                id: 5,
                                name: "Explore".into(),
                                target: None,
                                children: Some(vec![
                                    star(6, "Portable folder", Target::Folder { path: ".".into() }),
                                    star(
                                        7,
                                        "GitHub",
                                        Target::Url {
                                            url: "https://github.com/CorynBunnydeer/Constellation"
                                                .into(),
                                        },
                                    ),
                                ]),
                            },
                        ]),
                    },
                    star(8, "Files", Target::Folder { path: ".".into() }),
                    star(
                        9,
                        "Calculator",
                        Target::Program {
                            path: "calc.exe".into(),
                            args: vec![],
                            cwd: None,
                        },
                    ),
                ]),
            }],
            profiles: vec![Profile {
                id: 1,
                name: "Default".into(),
                binding: "Ctrl+Q".into(),
                mode: Mode::Click,
                hover: true,
                cursor_end: CursorEnd::Restore,
                directional: true,
                dot_cursor: true,
                universe: 0,
                root_id: None,
            }],
            layout: Layout::default(),
            sound: Sound::default(),
        }
    }
}
