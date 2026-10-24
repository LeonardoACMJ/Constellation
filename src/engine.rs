use crate::{
    config::{Config, Mode},
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
    fn rebuild(&mut self, c: &Config) {
        self.sectors = geometry::visible(c, &self.path);
    }
    pub fn motion(&mut self, c: &Config, p: Point) -> Action {
        let Some(profile) = self.profile.map(|i| &c.profiles[i]) else {
            return Action::None;
        };
        // The root hub remains neutral even if an offset child fan crosses it.
        let item = if p.x.hypot(p.y) < c.layout.radius * 0.30 {
            None
        } else if profile.directional {
            geometry::directional_hit(&self.sectors, p).cloned()
        } else {
            geometry::hit(&self.sectors, p).cloned()
        };
        let id = item.as_ref().map(|s| s.id);
        if id == self.hovered {
            return Action::None;
        }
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
        Action::Changed
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
