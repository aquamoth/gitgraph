//! Scripted runs for checking the rendering without a human: take a screenshot after a few
//! frames, optionally after dragging a node, then exit.

use std::path::PathBuf;

use eframe::egui::{self, Rect, Vec2};
use gitgraph_core::physics::NetParams;

use crate::scene::{Scene, to_point};
use crate::view::View;

#[derive(Debug, Default)]
pub struct Automation {
    /// Save a PNG of the window here, then exit.
    pub screenshot: Option<PathBuf>,
    /// Start with the whole graph fitted instead of at HEAD.
    pub fit: bool,
    /// Drag the node nearest the centre by this much before the screenshot (in the drag mode
    /// of the settings).
    pub demo_drag: Option<Vec2>,
    /// Zoom to apply (around the canvas centre) after the initial view is set up.
    pub zoom: Option<f32>,
    frame: u32,
    requested: bool,
    frame_times: Vec<std::time::Instant>,
    dragging: Option<(usize, egui::Pos2)>,
}

impl Automation {
    pub fn new(
        screenshot: Option<PathBuf>,
        fit: bool,
        demo_drag: Option<Vec2>,
        zoom: Option<f32>,
    ) -> Automation {
        Automation {
            screenshot,
            fit,
            demo_drag,
            zoom,
            ..Default::default()
        }
    }
}

const DRAG_START: u32 = 5;
const DRAG_FRAMES: u32 = 30;
const SETTLE_FRAMES: u32 = 90;

impl Automation {
    pub fn is_active(&self) -> bool {
        self.screenshot.is_some() || self.demo_drag.is_some()
    }

    /// Automated runs step the physics at a fixed rate so they are reproducible.
    pub fn fixed_dt(&self) -> Option<f32> {
        self.is_active().then_some(1.0 / 60.0)
    }

    pub fn drive(
        &mut self,
        ctx: &egui::Context,
        scene: &mut Scene,
        view: &mut View,
        canvas: Rect,
        params: &NetParams,
    ) {
        if !self.is_active() {
            return;
        }
        self.frame += 1;
        self.frame_times.push(std::time::Instant::now());
        ctx.request_repaint();
        if self.frame == 2
            && let Some(z) = self.zoom
        {
            view.zoom_around(canvas, canvas.center(), z / view.zoom);
        }

        if let Some(delta) = self.demo_drag {
            let f = self.frame;
            if f == DRAG_START {
                let centre = view.to_world(canvas, canvas.center());
                let node = (0..scene.node_count()).min_by(|&a, &b| {
                    scene
                        .node_center(a)
                        .distance_sq(centre)
                        .total_cmp(&scene.node_center(b).distance_sq(centre))
                });
                if let Some(n) = node {
                    let carried = scene.carried_nodes(&[n], params.model);
                    scene.net.grab(n, &[n], &carried, params.model.adapts());
                    self.dragging = Some((n, scene.node_center(n)));
                }
            } else if let Some((_, start)) = self.dragging {
                if f <= DRAG_START + DRAG_FRAMES {
                    let t = (f - DRAG_START) as f32 / DRAG_FRAMES as f32;
                    scene.net.drag_to(to_point(start + delta * t));
                } else if f == DRAG_START + DRAG_FRAMES + 1 {
                    scene.net.release(params);
                }
            }
        }

        let shoot_at = if self.demo_drag.is_some() {
            DRAG_START + DRAG_FRAMES + SETTLE_FRAMES
        } else {
            8
        };
        if self.frame >= shoot_at && !self.requested {
            self.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let gaps: Vec<f32> = self
                .frame_times
                .windows(2)
                .skip(3)
                .map(|w| (w[1] - w[0]).as_secs_f32() * 1000.0)
                .collect();
            if !gaps.is_empty() {
                let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
                let max = gaps.iter().copied().fold(0.0, f32::max);
                eprintln!(
                    "frame interval: mean {mean:.1} ms, max {max:.1} ms over {} frames",
                    gaps.len()
                );
            }
            if let Some(path) = &self.screenshot {
                let [w, h] = image.size;
                let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                match image::RgbaImage::from_raw(w as u32, h as u32, bytes)
                    .map(|img| img.save(path))
                {
                    Some(Ok(())) => eprintln!("saved screenshot to {}", path.display()),
                    Some(Err(e)) => eprintln!("could not save screenshot: {e}"),
                    None => eprintln!("screenshot had an unexpected size"),
                }
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
