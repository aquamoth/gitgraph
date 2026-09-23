//! Pan and zoom: the mapping between world (layout) coordinates and the screen.

use eframe::egui::{Pos2, Rect, Vec2, pos2};

pub const MIN_ZOOM: f32 = 0.02;
pub const MAX_ZOOM: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// World coordinate shown at the top-left corner of the canvas.
    pub offset: Vec2,
    pub zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        View {
            offset: Vec2::ZERO,
            zoom: 1.0,
        }
    }
}

impl View {
    pub fn to_screen(self, canvas: Rect, world: Pos2) -> Pos2 {
        canvas.min + (world.to_vec2() - self.offset) * self.zoom
    }

    pub fn to_world(self, canvas: Rect, screen: Pos2) -> Pos2 {
        pos2(0.0, 0.0) + self.offset + (screen - canvas.min) / self.zoom
    }

    pub fn rect_to_screen(self, canvas: Rect, world: Rect) -> Rect {
        Rect::from_min_max(
            self.to_screen(canvas, world.min),
            self.to_screen(canvas, world.max),
        )
    }

    /// The part of the world currently visible.
    pub fn visible_world(self, canvas: Rect) -> Rect {
        Rect::from_min_max(
            self.to_world(canvas, canvas.min),
            self.to_world(canvas, canvas.max),
        )
    }

    /// Multiplies the zoom by `factor`, keeping the world point under `anchor` fixed.
    pub fn zoom_around(&mut self, canvas: Rect, anchor: Pos2, factor: f32) {
        let world = self.to_world(canvas, anchor);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.offset = world.to_vec2() - (anchor - canvas.min) / self.zoom;
    }

    pub fn pan_screen(&mut self, delta: Vec2) {
        self.offset -= delta / self.zoom;
    }

    /// Places `world` at the given fraction of the canvas (0.5, 0.5 = centre).
    pub fn show_at(&mut self, canvas: Rect, world: Pos2, fraction: Vec2) {
        let screen_offset = canvas.size() * fraction;
        self.offset = world.to_vec2() - screen_offset / self.zoom;
    }

    /// Zooms and pans so `world` fills the canvas, never zooming in beyond `max_zoom`.
    pub fn fit(&mut self, canvas: Rect, world: Rect, max_zoom: f32) {
        let margin = 24.0;
        let avail = (canvas.size() - Vec2::splat(2.0 * margin)).max(Vec2::splat(1.0));
        let size = world.size().max(Vec2::splat(1.0));
        self.zoom = (avail.x / size.x)
            .min(avail.y / size.y)
            .clamp(MIN_ZOOM, max_zoom.min(MAX_ZOOM));
        self.show_at(canvas, world.center(), Vec2::splat(0.5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas() -> Rect {
        Rect::from_min_size(pos2(10.0, 20.0), Vec2::new(800.0, 600.0))
    }

    #[test]
    fn screen_and_world_roundtrip() {
        let v = View {
            offset: Vec2::new(100.0, -50.0),
            zoom: 0.5,
        };
        let w = pos2(123.0, 456.0);
        let back = v.to_world(canvas(), v.to_screen(canvas(), w));
        assert!((back - w).length() < 1e-3);
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let mut v = View::default();
        let anchor = pos2(300.0, 200.0);
        let before = v.to_world(canvas(), anchor);
        v.zoom_around(canvas(), anchor, 1.7);
        assert!((v.to_world(canvas(), anchor) - before).length() < 1e-3);
    }

    #[test]
    fn fit_shows_everything() {
        let mut v = View::default();
        let world = Rect::from_min_max(pos2(-500.0, 0.0), pos2(2500.0, 9000.0));
        v.fit(canvas(), world, 1.0);
        assert!(canvas().contains_rect(v.rect_to_screen(canvas(), world)));
    }
}
