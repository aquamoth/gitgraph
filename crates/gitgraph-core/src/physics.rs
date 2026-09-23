//! "Spider web" interaction: drag any node and the rest of the graph follows.
//!
//! Every node and every edge bend point is a particle. Particles are tied together by springs
//! that remember their offset in the original layout, along every edge and (optionally) between
//! neighbours within a layer, so the drawing behaves like a woven net. Each particle is also
//! weakly anchored to its home position, which limits how far a pull spreads. Nodes that are
//! dropped somewhere become pinned there; the net settles around them.
//!
//! The simulation is position-based dynamics (Müller et al. 2007): unconditionally stable,
//! cheap, and easy to tune. [`DragModel`] selects between prototypes of the behaviour.

use serde::{Deserialize, Serialize};

use crate::layout::{Layout, Point};

/// How the rest of the graph reacts when a node is dragged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DragModel {
    /// Springs along edges and across layers, with inertia: the net stretches and wobbles.
    #[default]
    Net,
    /// Springs along edges only, critically damped: neighbours follow smoothly, no wobble.
    Strings,
    /// Only the dragged node moves; edges re-route to it.
    Rigid,
}

impl DragModel {
    pub const ALL: [DragModel; 3] = [DragModel::Net, DragModel::Strings, DragModel::Rigid];

    pub fn label(self) -> &'static str {
        match self {
            DragModel::Net => "Spider web (springy net)",
            DragModel::Strings => "Strings (smooth follow)",
            DragModel::Rigid => "Rigid (move one node)",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetParams {
    pub model: DragModel,
    /// 0..=1: how far a pull spreads through the graph.
    pub reach: f32,
    /// 0..=1: how much the net overshoots and wobbles before settling (Net model only).
    pub wobble: f32,
    /// Push overlapping nodes apart.
    pub avoid_overlap: bool,
}

impl Default for NetParams {
    fn default() -> Self {
        NetParams {
            model: DragModel::Net,
            reach: 0.6,
            wobble: 0.4,
            avoid_overlap: true,
        }
    }
}

const SUBSTEPS: usize = 4;
const ITERATIONS: usize = 4;
/// Minimum gap kept between node boxes when avoiding overlap.
const OVERLAP_MARGIN: f32 = 6.0;
/// The simulation sleeps once no particle moves faster than this (layout units per second).
const SLEEP_SPEED: f32 = 1.0;

#[derive(Clone, Copy, Debug)]
struct Spring {
    a: u32,
    b: u32,
    /// Offset of `b` from `a` at rest.
    offset: Point,
    stiffness: f32,
    /// True for springs along edges, false for the springs between neighbours in a layer.
    along_edge: bool,
}

#[derive(Clone, Debug)]
pub struct Net {
    node_count: usize,
    /// Home position of every particle: the layout position, or where a node was dropped.
    home: Vec<Point>,
    pos: Vec<Point>,
    vel: Vec<Point>,
    /// Half extents of node boxes (zero for bend points).
    half: Vec<Point>,
    pinned: Vec<bool>,
    springs: Vec<Spring>,
    /// Particles of every edge, child node first, parent node last.
    chains: Vec<Vec<u32>>,
    grabbed: Option<(u32, Point)>,
    awake: bool,
    grid: Grid,
}

impl Net {
    /// Builds the net for a layout. `sizes` are the node box sizes.
    pub fn new(layout: &Layout, sizes: &[Point]) -> Net {
        let n = layout.nodes.len();
        let mut home = layout.nodes.clone();
        let mut half: Vec<Point> = sizes
            .iter()
            .map(|s| Point::new(s.x / 2.0, s.y / 2.0))
            .collect();
        // (layer, particle) of every particle, to link neighbours within a layer.
        let mut layer_of: Vec<u32> = layout.layers.clone();
        let mut chains = Vec::with_capacity(layout.edges.len());
        let mut springs = Vec::new();

        for (e, pts) in layout.edges.iter().enumerate() {
            let (child, parent) = layout.edge_ends[e];
            let mut chain = vec![child];
            for (k, p) in pts[1..pts.len() - 1].iter().enumerate() {
                chain.push(home.len() as u32);
                home.push(*p);
                half.push(Point::default());
                layer_of.push(layout.layers[child as usize] + 1 + k as u32);
            }
            chain.push(parent);
            for w in chain.windows(2) {
                springs.push(Spring {
                    a: w[0],
                    b: w[1],
                    offset: sub(home[w[1] as usize], home[w[0] as usize]),
                    stiffness: 1.0,
                    along_edge: true,
                });
            }
            chains.push(chain);
        }

        // Link each particle to its nearest neighbours within its layer: the "weft" of the net.
        let layer_count = layer_of.iter().map(|&l| l as usize + 1).max().unwrap_or(0);
        let mut by_layer: Vec<Vec<u32>> = vec![Vec::new(); layer_count];
        for (i, &l) in layer_of.iter().enumerate() {
            by_layer[l as usize].push(i as u32);
        }
        let vertical = layout.direction.is_vertical();
        let along = |p: Point| if vertical { p.x } else { p.y };
        for layer in &mut by_layer {
            layer.sort_by(|&a, &b| along(home[a as usize]).total_cmp(&along(home[b as usize])));
            for w in layer.windows(2) {
                springs.push(Spring {
                    a: w[0],
                    b: w[1],
                    offset: sub(home[w[1] as usize], home[w[0] as usize]),
                    stiffness: 0.5,
                    along_edge: false,
                });
            }
        }

        let count = home.len();
        Net {
            node_count: n,
            pos: home.clone(),
            home,
            vel: vec![Point::default(); count],
            half,
            pinned: vec![false; count],
            springs,
            chains,
            grabbed: None,
            awake: false,
            grid: Grid::default(),
        }
    }

    pub fn node_count(&self) -> usize {
        self.node_count
    }

    pub fn node_pos(&self, node: usize) -> Point {
        self.pos[node]
    }

    /// Current points of edge `e`: child centre, bend points, parent centre.
    pub fn edge_points(&self, e: usize) -> impl ExactSizeIterator<Item = Point> + '_ {
        self.chains[e].iter().map(|&p| self.pos[p as usize])
    }

    pub fn is_pinned(&self, node: usize) -> bool {
        self.pinned[node]
    }

    pub fn any_pinned(&self) -> bool {
        self.pinned[..self.node_count].iter().any(|&p| p)
    }

    /// True while the simulation still has motion to show.
    pub fn is_awake(&self) -> bool {
        self.awake || self.grabbed.is_some()
    }

    pub fn grabbed(&self) -> Option<usize> {
        self.grabbed.map(|(p, _)| p as usize)
    }

    /// Starts dragging `node`.
    pub fn grab(&mut self, node: usize) {
        self.grabbed = Some((node as u32, self.pos[node]));
        self.awake = true;
    }

    /// Moves the dragged node's target to `target` (a node centre position).
    pub fn drag_to(&mut self, target: Point) {
        if let Some((p, _)) = self.grabbed {
            self.grabbed = Some((p, target));
            self.awake = true;
        }
    }

    /// Drops the dragged node where it is and pins it there.
    pub fn release(&mut self) {
        if let Some((p, target)) = self.grabbed.take() {
            let p = p as usize;
            self.home[p] = target;
            self.pos[p] = target;
            self.vel[p] = Point::default();
            self.pinned[p] = true;
            self.awake = true;
        }
    }

    /// Lets a pinned node spring back to its layout position.
    pub fn unpin(&mut self, node: usize, layout_pos: Point) {
        self.pinned[node] = false;
        self.home[node] = layout_pos;
        self.awake = true;
    }

    /// Unpins everything; the whole net springs back to the layout.
    pub fn reset(&mut self, layout: &Layout) {
        for (i, p) in layout.nodes.iter().enumerate() {
            self.pinned[i] = false;
            self.home[i] = *p;
        }
        self.awake = true;
    }

    /// Advances the simulation by `dt` seconds. Returns true while anything is still moving.
    pub fn step(&mut self, dt: f32, params: &NetParams) -> bool {
        if !self.is_awake() {
            return false;
        }
        let dt = dt.clamp(1.0 / 240.0, 1.0 / 30.0);
        let h = dt / SUBSTEPS as f32;
        // In a chain of springs with anchors, a displacement decays by a factor λ per hop where
        // anchor/spring ≈ (1 − λ)². `reach` picks λ between 0.3 (local) and 0.9 (far-reaching).
        let lambda = 0.3 + 0.6 * params.reach.clamp(0.0, 1.0);
        let anchor = (1.0 - lambda) * (1.0 - lambda);
        let (damping, use_layer_springs, rigid) = match params.model {
            DragModel::Net => (
                0.02 + 0.2 * (1.0 - params.wobble.clamp(0.0, 1.0)),
                true,
                false,
            ),
            DragModel::Strings => (0.45, false, false),
            DragModel::Rigid => (1.0, false, true),
        };

        let mut pred = self.pos.clone();
        let mut max_speed = 0.0f32;
        for _ in 0..SUBSTEPS {
            for ((v, p), x) in self.vel.iter_mut().zip(&mut pred).zip(&self.pos) {
                *v = scale(*v, 1.0 - damping);
                *p = add(*x, scale(*v, h));
            }
            for _ in 0..ITERATIONS {
                self.fix(&mut pred);
                if rigid {
                    self.pull_to_home(&mut pred, 1.0);
                    continue;
                }
                for s in &self.springs {
                    if !s.along_edge && !use_layer_springs {
                        continue;
                    }
                    let (a, b) = (s.a as usize, s.b as usize);
                    let (wa, wb) = (self.inv_mass(a), self.inv_mass(b));
                    if wa + wb == 0.0 {
                        continue;
                    }
                    let err = sub(sub(pred[b], pred[a]), s.offset);
                    let stiffness = if s.along_edge {
                        s.stiffness
                    } else if dot(err, s.offset) < 0.0 {
                        // Neighbours in a layer pushed together: keep them apart.
                        s.stiffness
                    } else {
                        // Pulled apart: only a faint pull, so the net does not move as a sheet.
                        s.stiffness * 0.1
                    };
                    let corr = scale(err, stiffness / (wa + wb));
                    pred[a] = add(pred[a], scale(corr, wa));
                    pred[b] = sub(pred[b], scale(corr, wb));
                }
                self.pull_to_home(&mut pred, anchor);
            }
            if params.avoid_overlap {
                self.separate_nodes(&mut pred);
            }
            self.fix(&mut pred);
            for ((v, x), p) in self.vel.iter_mut().zip(&mut self.pos).zip(&pred) {
                *v = scale(sub(*p, *x), 1.0 / h);
                max_speed = max_speed.max(len(*v));
                *x = *p;
            }
        }
        self.awake = max_speed > SLEEP_SPEED;
        if !self.awake && self.grabbed.is_none() {
            self.vel.iter_mut().for_each(|v| *v = Point::default());
        }
        self.is_awake()
    }

    fn inv_mass(&self, i: usize) -> f32 {
        if self.pinned[i] || self.grabbed.is_some_and(|(g, _)| g as usize == i) {
            0.0
        } else if i < self.node_count {
            1.0
        } else {
            // Bend points are light so edges bend before nodes move.
            2.0
        }
    }

    /// Holds pinned and grabbed particles in place.
    fn fix(&self, pred: &mut [Point]) {
        for (i, p) in pred.iter_mut().enumerate().take(self.node_count) {
            if self.pinned[i] {
                *p = self.home[i];
            }
        }
        if let Some((g, target)) = self.grabbed {
            pred[g as usize] = target;
        }
    }

    fn pull_to_home(&self, pred: &mut [Point], strength: f32) {
        for (i, p) in pred.iter_mut().enumerate() {
            if self.inv_mass(i) > 0.0 {
                *p = add(*p, scale(sub(self.home[i], *p), strength));
            }
        }
    }

    /// Pushes overlapping node boxes apart along their axis of least overlap.
    fn separate_nodes(&mut self, pred: &mut [Point]) {
        let n = self.node_count;
        self.grid.rebuild(&pred[..n], &self.half[..n]);
        let mut pairs = Vec::new();
        self.grid.candidate_pairs(&mut pairs);
        for (a, b) in pairs {
            let (a, b) = (a as usize, b as usize);
            let d = sub(pred[b], pred[a]);
            let ox = self.half[a].x + self.half[b].x + OVERLAP_MARGIN - d.x.abs();
            let oy = self.half[a].y + self.half[b].y + OVERLAP_MARGIN - d.y.abs();
            if ox <= 0.0 || oy <= 0.0 {
                continue;
            }
            let (wa, wb) = (self.inv_mass(a), self.inv_mass(b));
            if wa + wb == 0.0 {
                continue;
            }
            let push = if ox < oy {
                Point::new(if d.x < 0.0 { -ox } else { ox }, 0.0)
            } else {
                Point::new(0.0, if d.y < 0.0 { -oy } else { oy })
            };
            pred[a] = sub(pred[a], scale(push, wa / (wa + wb)));
            pred[b] = add(pred[b], scale(push, wb / (wa + wb)));
        }
    }
}

/// Uniform grid for finding overlapping node boxes.
#[derive(Clone, Debug, Default)]
struct Grid {
    cell: f32,
    cells: std::collections::HashMap<(i32, i32), Vec<u32>>,
}

impl Grid {
    fn rebuild(&mut self, pos: &[Point], half: &[Point]) {
        self.cells.clear();
        let max_half = half.iter().fold(0.0f32, |m, h| m.max(h.x).max(h.y));
        self.cell = (2.0 * max_half + OVERLAP_MARGIN).max(32.0);
        for (i, (p, h)) in pos.iter().zip(half).enumerate() {
            let (x0, y0) = self.key(p.x - h.x, p.y - h.y);
            let (x1, y1) = self.key(p.x + h.x, p.y + h.y);
            for x in x0..=x1 {
                for y in y0..=y1 {
                    self.cells.entry((x, y)).or_default().push(i as u32);
                }
            }
        }
    }

    fn key(&self, x: f32, y: f32) -> (i32, i32) {
        (
            (x / self.cell).floor() as i32,
            (y / self.cell).floor() as i32,
        )
    }

    fn candidate_pairs(&self, out: &mut Vec<(u32, u32)>) {
        for items in self.cells.values() {
            for (i, &a) in items.iter().enumerate() {
                for &b in &items[i + 1..] {
                    out.push((a.min(b), a.max(b)));
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }
}

fn add(a: Point, b: Point) -> Point {
    Point::new(a.x + b.x, a.y + b.y)
}

fn sub(a: Point, b: Point) -> Point {
    Point::new(a.x - b.x, a.y - b.y)
}

fn scale(a: Point, s: f32) -> Point {
    Point::new(a.x * s, a.y * s)
}

fn dot(a: Point, b: Point) -> f32 {
    a.x * b.x + a.y * b.y
}

fn len(a: Point) -> f32 {
    (a.x * a.x + a.y * a.y).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{self, LayoutEdge, LayoutInput, LayoutOptions};

    /// A chain of five nodes: 0 -> 1 -> 2 -> 3 -> 4.
    fn chain_net() -> (Layout, Net) {
        let input = LayoutInput {
            sizes: vec![Point::new(60.0, 20.0); 5],
            times: vec![5, 4, 3, 2, 1],
            edges: (0..4)
                .map(|i| LayoutEdge {
                    child: i,
                    parent: i + 1,
                    first_parent: true,
                })
                .collect(),
            priority: Vec::new(),
        };
        let l = layout::layout(&input, &LayoutOptions::default());
        let net = Net::new(&l, &input.sizes);
        (l, net)
    }

    fn settle(net: &mut Net, params: &NetParams) {
        for _ in 0..2000 {
            if !net.step(1.0 / 60.0, params) {
                return;
            }
        }
        panic!("net did not settle");
    }

    #[test]
    fn pull_spreads_with_decay_and_pins_on_release() {
        for model in [DragModel::Net, DragModel::Strings] {
            let (l, mut net) = chain_net();
            let params = NetParams {
                model,
                ..NetParams::default()
            };
            net.grab(2);
            let target = Point::new(l.nodes[2].x + 200.0, l.nodes[2].y);
            net.drag_to(target);
            for _ in 0..30 {
                net.step(1.0 / 60.0, &params);
            }
            net.release();
            settle(&mut net, &params);
            assert_eq!(net.node_pos(2), target, "{model:?}: dropped node stays put");
            assert!(net.is_pinned(2));
            let moved = |i: usize| net.node_pos(i).x - l.nodes[i].x;
            assert!(
                moved(1) > 20.0 && moved(3) > 20.0,
                "{model:?}: neighbours follow"
            );
            assert!(
                moved(1) < 200.0 && moved(0) < moved(1),
                "{model:?}: pull decays"
            );
        }
    }

    #[test]
    fn rigid_moves_only_the_dragged_node() {
        let (l, mut net) = chain_net();
        let params = NetParams {
            model: DragModel::Rigid,
            ..NetParams::default()
        };
        net.grab(2);
        net.drag_to(Point::new(500.0, 0.0));
        net.release();
        settle(&mut net, &params);
        for i in [0, 1, 3, 4] {
            assert!((net.node_pos(i).x - l.nodes[i].x).abs() < 0.5);
        }
    }

    #[test]
    fn reset_returns_to_layout() {
        let (l, mut net) = chain_net();
        let params = NetParams::default();
        net.grab(1);
        net.drag_to(Point::new(-300.0, 50.0));
        net.release();
        settle(&mut net, &params);
        net.reset(&l);
        settle(&mut net, &params);
        for i in 0..5 {
            let p = net.node_pos(i);
            assert!((p.x - l.nodes[i].x).abs() < 1.0 && (p.y - l.nodes[i].y).abs() < 1.0);
        }
    }
}
