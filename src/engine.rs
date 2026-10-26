use crate::{
    config::{Config, Mode, Node},
    geometry::{self, Point, Sector},
};

#[derive(Debug, Default)]
pub struct Engine {
    pub profile: Option<usize>,
    pub path: Vec<u64>,
    pub hovered: Option<u64>,
    pub sectors: Vec<Sector>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Changed,
    Launch(u64),
}
impl Engine {
    pub fn open(&mut self, index: usize, c: &Config) -> bool {
        if self.profile.is_some() || index >= c.profiles.len() {
            return false;
        }
        self.profile = Some(index);
        self.path.clear();
        self.hovered = None;
        self.rebuild(c);
        true
    }
    pub fn close(&mut self) {
        self.profile = None;
        self.path.clear();
        self.hovered = None;
        self.sectors.clear();
    }
    /// Where the open profile's radial starts: its universe root, or its own
    /// entry node. None while nothing is open.
    pub fn entry<'a>(&self, c: &'a Config) -> Option<&'a Node> {
        c.entry_of(c.profiles.get(self.profile?)?)
    }
    fn rebuild(&mut self, c: &Config) {
        let profile = self.profile.and_then(|i| c.profiles.get(i)).map(|p| p.id);
        self.sectors = match self.entry(c) {
            Some(start) => geometry::visible_from(c, start, profile, &self.path),
            None => vec![],
        };
    }
    /// Keep an open fan's approach corridor from selecting an older-level
    /// sibling through its inner hole. This is spatial hysteresis, not a timer.
    fn protected_by_open_fan(&self, c: &Config, candidate: &Sector, p: Point) -> bool {
        use std::f64::consts::{PI, TAU};
        if self.path.contains(&candidate.id) {
            return false;
        }
        let spread = c.layout.spread.to_radians();
        // A full circle has no angular exit edge; leave its hit testing alone.
        if spread >= TAU {
            return false;
        }
        let margin = 8_f64.to_radians().min((TAU - spread) / 4.);
        self.path
            .iter()
            .enumerate()
            .skip(candidate.depth)
            .any(|(depth, id)| {
                let Some(parent) = self
                    .sectors
                    .iter()
                    .find(|s| s.depth == depth && s.id == *id)
                else {
                    return false;
                };
                let Some(child) = self
                    .sectors
                    .iter()
                    .find(|s| s.depth == depth + 1 && s.sweep > 0.)
                else {
                    return false;
                };
                let angle = (p.y - child.center.y).atan2(p.x - child.center.x);
                let delta = (angle - parent.angle + PI).rem_euclid(TAU) - PI;
                delta.abs() <= spread / 2. + margin
            })
    }
    pub fn motion(&mut self, c: &Config, p: Point) -> Action {
        self.motion_with_feedback(c, p).0
    }
    /// The second result is true only when the hovered item gains a light.
    /// Ancestors already illuminated in the active path do not gain it again.
    pub fn motion_with_feedback(&mut self, c: &Config, p: Point) -> (Action, bool) {
        let Some(profile) = self.profile.map(|i| &c.profiles[i]) else {
            return (Action::None, false);
        };
        // The root hub remains neutral even if an offset child fan crosses it.
        let item = if p.x.hypot(p.y) < c.layout.radius * 0.30 {
            None
        } else if profile.directional {
            geometry::directional_hit(&self.sectors, p).cloned()
        } else {
            geometry::hit(&self.sectors, p).cloned()
        }
        .filter(|s| !self.protected_by_open_fan(c, s, p));
        let id = item.as_ref().map(|s| s.id);
        if id == self.hovered {
            return (Action::None, false);
        }
        let new_highlight = id.is_some_and(|id| !self.path.contains(&id));
        self.hovered = id;
        if let Some(s) = item {
            if c.find(s.id).is_some_and(|n| n.children.is_some()) {
                if profile.hover && self.path.get(s.depth) != Some(&s.id) {
                    self.path.truncate(s.depth);
                    self.path.push(s.id);
                    self.rebuild(c);
                }
            } else if self.path.len() > s.depth {
                self.path.truncate(s.depth);
                self.rebuild(c);
            }
        }
        (Action::Changed, new_highlight)
    }
    pub fn select(&mut self, c: &Config) -> Action {
        if self.profile.is_none() {
            return Action::None;
        }
        let Some(id) = self.hovered else {
            return Action::None;
        };
        let Some(n) = c.find(id) else {
            return Action::None;
        };
        if n.children.is_none() {
            return Action::Launch(id);
        }
        let Some(s) = self.sectors.iter().find(|s| s.id == id).cloned() else {
            return Action::None;
        };
        if self.path.get(s.depth) != Some(&id) {
            self.path.truncate(s.depth);
            self.path.push(id);
            self.rebuild(c);
            return Action::Changed;
        }
        Action::None
    }
    pub fn release(&mut self, c: &Config) -> Action {
        let Some(i) = self.profile else {
            return Action::None;
        };
        if c.profiles[i].mode != Mode::Hold {
            return Action::None;
        }
        let action = self
            .hovered
            .filter(|id| c.find(*id).is_some_and(|n| n.target.is_some()))
            .map(Action::Launch)
            .unwrap_or(Action::Changed);
        if !matches!(action, Action::Launch(_)) {
            self.close()
        }
        action
    }
    pub fn back(&mut self, c: &Config) {
        self.path.pop();
        self.hovered = None;
        self.rebuild(c);
    }
    pub fn root(&mut self, c: &Config) {
        self.path.clear();
        self.hovered = None;
        self.rebuild(c);
    }
}
