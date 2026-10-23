use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub root: Node,
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub layout: Layout,
    #[serde(default)]
    pub sound: Sound,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: u64,
    pub name: String,
    /// A node is either a constellation (Some children, including empty), or a Star.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<Node>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
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
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[serde(default)]
    pub id: u64,
    pub name: String,
    pub binding: String,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default = "yes")]
    pub hover: bool,
    #[serde(default = "yes")]
    pub restore_cursor: bool,
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
    pub enabled: bool,
    pub volume: f64,
}
impl Default for Sound {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: 0.18,
        }
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
        if self.root.children.is_none() {
            return Err("Root must be a constellation".into());
        }
        let mut ids = HashSet::new();
        fn walk(n: &Node, ids: &mut HashSet<u64>, depth: usize) -> Result<(), String> {
            if depth > 64 {
                return Err("Configuration nesting exceeds parser safety limit (64)".into());
            }
            if !ids.insert(n.id) {
                return Err(format!("Duplicate node ID {}", n.id));
            }
            if n.name.trim().is_empty() {
                return Err("Every node needs a name".into());
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
        walk(&self.root, &mut ids, 0)?;
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
        }
        Ok(())
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
        self.root.find(id)
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
                id,
                name: name.into(),
                children: None,
                target: Some(target),
            }
        }
        Self {
            version: 1,
            root: Node {
                id: 1,
                name: "Your universe".into(),
                target: None,
                children: Some(vec![
                    Node {
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
            },
            profiles: vec![Profile {
                id: 1,
                name: "Default".into(),
                binding: "Ctrl+Q".into(),
                mode: Mode::Click,
                hover: true,
                restore_cursor: true,
            }],
            layout: Layout::default(),
            sound: Sound::default(),
        }
    }
}
