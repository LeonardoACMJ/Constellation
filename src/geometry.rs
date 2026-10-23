use crate::config::{Config, Layout, Node};
use std::f64::consts::{PI, TAU};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
impl Point {
    pub fn polar(self, r: f64, a: f64) -> Self {
        Self {
            x: self.x + r * a.cos(),
            y: self.y + r * a.sin(),
        }
    }
}
#[derive(Debug, Clone)]
pub struct Sector {
    pub id: u64,
    pub depth: usize,
    pub center: Point,
    pub inner: f64,
    pub outer: f64,
    pub from: f64,
    pub sweep: f64,
    pub angle: f64,
    pub dot: Point,
}
impl Sector {
    pub fn contains(&self, p: Point) -> bool {
        let dx = p.x - self.center.x;
        let dy = p.y - self.center.y;
        let r = dx.hypot(dy);
        if r < self.inner || r > self.outer {
            return false;
        }
        (dy.atan2(dx) - self.from).rem_euclid(TAU) <= self.sweep
    }
    pub fn polygon(&self) -> Vec<Point> {
        let steps = ((self.sweep * 35.).ceil() as usize).max(2);
        (0..=steps)
            .map(|i| {
                self.center
                    .polar(self.outer, self.from + self.sweep * i as f64 / steps as f64)
            })
            .chain((0..=steps).rev().map(|i| {
                self.center
                    .polar(self.inner, self.from + self.sweep * i as f64 / steps as f64)
            }))
            .collect()
    }
}

fn fan(node: &Node, depth: usize, center: Point, direction: f64, l: Layout, out: &mut Vec<Sector>) {
    let Some(children) = node.children.as_ref() else {
        return;
    };
    if children.is_empty() {
        return;
    }
    let sweep = if depth == 0 {
        TAU
    } else {
        l.spread.to_radians()
    };
    let step = sweep / children.len() as f64;
    let start = if depth == 0 {
        -PI / 2. - step / 2.
    } else {
        direction - sweep / 2.
    };
    // User gaps may intentionally hide target area. Do not invert polygon winding.
    let gap = (l.gap / l.radius).min(step);
    let inner = l.radius * if depth == 0 { 0.30 } else { 0.58 };
    let outer = l.radius * if depth == 0 { 1.0 } else { 1.15 };
    for (i, n) in children.iter().enumerate() {
        let angle = start + (i as f64 + 0.5) * step;
        out.push(Sector {
            id: n.id,
            depth,
            center,
            inner,
            outer,
            from: start + i as f64 * step + gap / 2.,
            sweep: (step - gap).max(0.),
            angle,
            dot: center.polar(inner + (outer - inner) * 0.5, angle),
        });
    }
}

pub fn visible(config: &Config, path: &[u64]) -> Vec<Sector> {
    let mut out = vec![];
    let mut node = &config.root;
    let mut center = Point::default();
    let mut direction = -PI / 2.;
    fan(node, 0, center, direction, config.layout, &mut out);
    for (depth, id) in path.iter().enumerate() {
        let Some(s) = out
            .iter()
            .find(|s| s.depth == depth && s.id == *id)
            .cloned()
        else {
            break;
        };
        let Some(child) = node
            .children
            .as_ref()
            .and_then(|cs| cs.iter().find(|n| n.id == *id))
        else {
            break;
        };
        if child.children.is_none() {
            break;
        }
        center = center.polar(config.layout.radius * config.layout.distance, s.angle);
        direction = s.angle;
        node = child;
        fan(node, depth + 1, center, direction, config.layout, &mut out);
    }
    out
}
pub fn full(config: &Config) -> Vec<Sector> {
    fn walk(
        node: &Node,
        depth: usize,
        center: Point,
        direction: f64,
        l: Layout,
        out: &mut Vec<Sector>,
    ) {
        let start = out.len();
        fan(node, depth, center, direction, l, out);
        let added = out[start..].to_vec();
        for (s, child) in added
            .iter()
            .zip(node.children.as_deref().unwrap_or_default())
        {
            if child.children.is_some() {
                walk(
                    child,
                    depth + 1,
                    center.polar(l.radius * l.distance, s.angle),
                    s.angle,
                    l,
                    out,
                );
            }
        }
    }
    let mut out = vec![];
    walk(
        &config.root,
        0,
        Point::default(),
        -PI / 2.,
        config.layout,
        &mut out,
    );
    out
}
pub fn hit(sectors: &[Sector], p: Point) -> Option<&Sector> {
    sectors.iter().rev().find(|s| s.sweep > 0. && s.contains(p))
}
pub fn warnings(config: &Config, width: f64, height: f64) -> Vec<String> {
    let sectors = full(config);
    let mut narrow = false;
    let mut overflow = false;
    for s in &sectors {
        narrow |= s.sweep * (s.inner + s.outer) * 0.5 < 24.;
        overflow |= s
            .polygon()
            .iter()
            .any(|p| p.x.abs() > width / 2. || p.y.abs() > height / 2.);
    }
    let mut result = vec![];
    if overflow {
        result.push(
            "Some branches extend outside this monitor's work area; saving is still allowed."
                .into(),
        )
    }
    if narrow {
        result.push("Some sectors are narrow or hidden by gaps; saving is still allowed.".into())
    }
    if preview_overlap(config, &sectors) {
        result
            .push("Some target areas overlap in the full preview; saving is still allowed.".into());
    }
    result
}

// Normal parent/descendant stacking is intentional. Report intersections between
// other target areas in the full preview, including mutually exclusive branches.
fn preview_overlap(config: &Config, sectors: &[Sector]) -> bool {
    use std::collections::HashMap;
    fn parents(node: &Node, out: &mut HashMap<u64, u64>) {
        if let Some(children) = &node.children {
            for c in children {
                out.insert(c.id, node.id);
                parents(c, out);
            }
        }
    }
    let mut links = HashMap::new();
    parents(&config.root, &mut links);
    let ancestor = |a: u64, mut b: u64| {
        while let Some(parent) = links.get(&b) {
            if *parent == a {
                return true;
            }
            b = *parent;
        }
        false
    };
    let shapes: Vec<_> = sectors
        .iter()
        .map(|s| {
            let p = s.polygon();
            let bounds = p.iter().fold(
                (
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ),
                |(x, y, r, b), p| (x.min(p.x), y.min(p.y), r.max(p.x), b.max(p.y)),
            );
            (p, bounds)
        })
        .collect();
    // Sectors in one fan cannot overlap one another. Group first so a flat
    // 10,000-item fan never pays for 50 million pointless pair comparisons.
    let mut groups = std::collections::BTreeMap::<u64, Vec<usize>>::new();
    for (i, s) in sectors.iter().enumerate().filter(|(_, s)| s.sweep > 0.) {
        groups.entry(links[&s.id]).or_default().push(i);
    }
    let groups: Vec<_> = groups.into_values().collect();
    for (gi, group) in groups.iter().enumerate() {
        for other in &groups[..gi] {
            for &i in group {
                for &j in other {
                    let (a, b) = (&sectors[i], &sectors[j]);
                    if ancestor(a.id, b.id) || ancestor(b.id, a.id) {
                        continue;
                    }
                    let (pa, ra) = &shapes[i];
                    let (pb, rb) = &shapes[j];
                    if ra.0 >= rb.2 || rb.0 >= ra.2 || ra.1 >= rb.3 || rb.1 >= ra.3 {
                        continue;
                    }
                    if a.contains(b.dot) || b.contains(a.dot) || polygons_intersect(pa, pb) {
                        return true;
                    }
                }
            }
        }
    }

    false
}
fn polygons_intersect(a: &[Point], b: &[Point]) -> bool {
    fn cross(a: Point, b: Point, c: Point) -> f64 {
        (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
    }
    let edges = |p: &[Point]| -> Vec<(Point, Point)> {
        p.iter()
            .copied()
            .zip(p.iter().copied().cycle().skip(1))
            .take(p.len())
            .collect()
    };
    let ae = edges(a);
    let be = edges(b);
    if ae.iter().any(|(a, b)| {
        be.iter().any(|(c, d)| {
            cross(*a, *b, *c) * cross(*a, *b, *d) < -1e-10
                && cross(*c, *d, *a) * cross(*c, *d, *b) < -1e-10
        })
    }) {
        return true;
    }
    fn inside(p: Point, edges: &[(Point, Point)]) -> bool {
        let mut inside = false;
        for (a, b) in edges {
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                inside = !inside;
            }
        }
        inside
    }
    a.first().is_some_and(|p| inside(*p, &be)) || b.first().is_some_and(|p| inside(*p, &ae))
}
