//! Latest-only geometry preparation; GDI painting stays on the owning UI thread.
use crate::{
    config::{Config, Layout},
    geometry::{self, Sector},
    theme::Palette,
};
use std::sync::Arc;
#[derive(Clone)]
pub struct Request {
    pub config: Arc<Config>,
    pub universe: usize,
    pub layout: Layout,
    pub theme: Palette,
    pub width: f64,
    pub height: f64,
}
impl PartialEq for Request {
    fn eq(&self, b: &Self) -> bool {
        Arc::ptr_eq(&self.config, &b.config)
            && self.universe == b.universe
            && self.layout == b.layout
            && self.theme == b.theme
            && self.width == b.width
            && self.height == b.height
    }
}
pub struct Prepared {
    pub config: Arc<Config>,
    pub universe: usize,
    pub sectors: Vec<Sector>,
    pub warnings: String,
    pub extent_x: f64,
    pub extent_y: f64,
    pub width: f64,
    pub height: f64,
    pub elapsed_ms: f64,
}
pub fn prepare(request: &Request) -> Arc<Prepared> {
    let then = std::time::Instant::now();
    let mut config = (*request.config).clone();
    config.layout = request.layout;
    config.theme = request.theme;
    let sectors = geometry::full_from(&config, &config.universes[request.universe], None);
    let (mut x, mut y) = (request.width / 2., request.height / 2.);
    for sector in &sectors {
        for p in sector.polygon() {
            x = x.max(p.x.abs());
            y = y.max(p.y.abs());
        }
    }
    let warnings = geometry::warnings_for_sectors(&config, &sectors, request.width, request.height);
    Arc::new(Prepared {
        config: Arc::new(config),
        universe: request.universe,
        sectors,
        warnings: if warnings.is_empty() {
            "Layout fits this monitor's work area.".into()
        } else {
            warnings.join(" ")
        },
        extent_x: x,
        extent_y: y,
        width: request.width,
        height: request.height,
        elapsed_ms: then.elapsed().as_secs_f64() * 1000.,
    })
}
