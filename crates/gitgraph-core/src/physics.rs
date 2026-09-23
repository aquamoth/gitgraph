//! "Spider web" interaction: drag any node and the rest of the graph follows.
//!
//! Every node and every edge bend point is a particle. Particles are tied together by springs
//! that remember their offset in the layout: along every edge, and between neighbours within a
//! layer (those only resist being pushed together), so the drawing behaves like a woven net.
//! Each particle is also weakly anchored to its layout position, which limits how far a pull
//! spreads. A node dropped somewhere stays pinned there and the net settles around it.
//!
//! The state is kept as *displacements from the layout*, which stay small and therefore exact
//! even where layout coordinates run into the millions. Each frame has two parts:
//!
//! 1. **Shape.** The net's resting shape for the current drag minimises
//!    `Σ k_s |d_b − d_a|² + Σ k_a |d_i|²` over displacements `d`, with the dragged and pinned
//!    nodes held fixed. It is found by Gauss-Seidel relaxation over the woken particles, warm-
//!    started from the previous frame. In a chain the displacement decays by a factor λ per hop
//!    when `k_a / k_s = (1 − λ)² / λ`, which is what [`NetParams::reach`] sets.
//! 2. **Motion.** Every particle follows its target through a damped spring, so the net moves
//!    with some inertia and, depending on [`NetParams::wobble`], overshoots a little.
//!
//! Only particles near the dragged node are simulated (breadth-first up to a budget), so
//! dragging stays smooth in graphs with a million bend points. [`DragModel`] selects between
//! prototypes of the behaviour.

use std::collections::{HashMap, HashSet, VecDeque};

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

/// At most this many particles are woken around a grabbed node; the rest of a huge graph
/// stays still (a pull has decayed to nothing long before that many hops).
const ACTIVE_BUDGET: usize = 8_000;
/// Gauss-Seidel sweeps over the woken particles per frame.
const SWEEPS: usize = 12;
/// Integration substeps per frame for following the targets.
const SUBSTEPS: usize = 4;
/// Natural frequency (Hz) with which particles follow their targets.
const FOLLOW_HZ: f32 = 3.0;
/// Minimum gap kept between node boxes when avoiding overlap.
const OVERLAP_MARGIN: f32 = 6.0;
/// The simulation sleeps once no particle moves faster than this (layout units per second)
/// and every particle is this close to its target.
const SLEEP_SPEED: f32 = 1.0;
const SLEEP_DISTANCE: f32 = 0.1;

#[derive(Clone, Copy, Debug)]
struct Spring {
    a: u32,
    b: u32,
    /// Offset of `b` from `a` in the layout.
    offset: Point,
    stiffness: f32,
    /// True for springs along edges, false for those between neighbours in a layer.
    along_edge: bool,
}

#[derive(Clone, Debug)]
pub struct Net {
    node_count: usize,
    /// Layout position of every particle.
    origin: Vec<Point>,
    /// Current displacement from the layout, its velocity, and where it is heading.
    disp: Vec<Point>,
    vel: Vec<Point>,
    target: Vec<Point>,
    /// Half extents of node boxes (zero for bend points).
    half: Vec<Point>,
    /// Layer of every node, and whether layers run horizontally (newest on top or bottom).
    node_layer: Vec<u32>,
    vertical: bool,
    /// Displacement a dropped node is pinned at.
    pinned: Vec<Option<Point>>,
    springs: Vec<Spring>,
    /// Spring ids per particle.
    adjacent: Vec<Vec<u32>>,
    /// Particles of every edge, child node first, parent node last.
    chains: Vec<Vec<u32>>,
    /// The dragged node and the displacement it is dragged to.
    grabbed: Option<(u32, Point)>,
    awake: bool,
    /// Particles being simulated; everything else is at rest.
    active: Vec<u32>,
    is_active: Vec<bool>,
    grid: Grid,
}

impl Net {
    /// Builds the net for a layout. `sizes` are the node box sizes.
    pub fn new(layout: &Layout, sizes: &[Point]) -> Net {
        let n = layout.nodes.len();
        let mut origin = layout.nodes.clone();
        let mut half: Vec<Point> = sizes
            .iter()
            .map(|s| Point::new(s.x / 2.0, s.y / 2.0))
            .collect();
        let mut layer_of: Vec<u32> = layout.layers.clone();
        let mut chains = Vec::with_capacity(layout.edges.len());
        let mut springs = Vec::new();
        let mut seen_segments = HashSet::new();
        // Bend point identity -> particle, so bundled edges share their particles.
        let mut bend_particle: HashMap<u32, u32> = HashMap::new();

        for (e, pts) in layout.edges.iter().enumerate() {
            let (child, parent) = layout.edge_ends[e];
            let mut chain = vec![child];
            for (k, p) in pts[1..pts.len() - 1].iter().enumerate() {
                let id = layout.edge_bends.get(e).and_then(|b| b.get(k)).copied();
                let particle = match id.and_then(|id| bend_particle.get(&id)) {
                    Some(&particle) => particle,
                    None => {
                        let particle = origin.len() as u32;
                        origin.push(*p);
                        half.push(Point::default());
                        layer_of.push(layout.layers[child as usize] + 1 + k as u32);
                        if let Some(id) = id {
                            bend_particle.insert(id, particle);
                        }
                        particle
                    }
                };
                chain.push(particle);
            }
            chain.push(parent);
            for w in chain.windows(2) {
                if seen_segments.insert((w[0], w[1])) {
                    springs.push(Spring {
                        a: w[0],
                        b: w[1],
                        offset: sub(origin[w[1] as usize], origin[w[0] as usize]),
                        stiffness: 1.0,
                        along_edge: true,
                    });
                }
            }
            chains.push(chain);
        }

        // Link each particle to its neighbours within its layer: the "weft" of the net.
        let layer_count = layer_of.iter().map(|&l| l as usize + 1).max().unwrap_or(0);
        let mut by_layer: Vec<Vec<u32>> = vec![Vec::new(); layer_count];
        for (i, &l) in layer_of.iter().enumerate() {
            by_layer[l as usize].push(i as u32);
        }
        let vertical = layout.direction.is_vertical();
        let along = |p: Point| if vertical { p.x } else { p.y };
        for layer in &mut by_layer {
            layer.sort_by(|&a, &b| along(origin[a as usize]).total_cmp(&along(origin[b as usize])));
            for w in layer.windows(2) {
                springs.push(Spring {
                    a: w[0],
                    b: w[1],
                    offset: sub(origin[w[1] as usize], origin[w[0] as usize]),
                    stiffness: 0.5,
                    along_edge: false,
                });
            }
        }

        let count = origin.len();
        let mut adjacent = vec![Vec::new(); count];
        for (i, s) in springs.iter().enumerate() {
            adjacent[s.a as usize].push(i as u32);
            adjacent[s.b as usize].push(i as u32);
        }
        Net {
            node_count: n,
            origin,
            disp: vec![Point::default(); count],
            vel: vec![Point::default(); count],
            target: vec![Point::default(); count],
            half,
            node_layer: layout.layers.clone(),
            vertical,
            pinned: vec![None; count],
            springs,
            adjacent,
            chains,
            grabbed: None,
            awake: false,
            active: Vec::new(),
            is_active: vec![false; count],
            grid: Grid::default(),
        }
    }

    pub fn node_count(&self) -> usize {
        self.node_count
    }

    fn pos(&self, i: usize) -> Point {
        add(self.origin[i], self.disp[i])
    }

    pub fn node_pos(&self, node: usize) -> Point {
        self.pos(node)
    }

    /// Current points of edge `e`: child centre, bend points, parent centre.
    pub fn edge_points(&self, e: usize) -> impl ExactSizeIterator<Item = Point> + '_ {
        self.chains[e].iter().map(|&p| self.pos(p as usize))
    }

    pub fn is_pinned(&self, node: usize) -> bool {
        self.pinned[node].is_some()
    }

    pub fn any_pinned(&self) -> bool {
        self.pinned[..self.node_count].iter().any(Option::is_some)
    }

    /// True while the simulation still has motion to show.
    pub fn is_awake(&self) -> bool {
        self.awake || self.grabbed.is_some()
    }

    pub fn grabbed(&self) -> Option<usize> {
        self.grabbed.map(|(p, _)| p as usize)
    }

    /// Number of particles currently simulated.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Starts dragging `node`.
    pub fn grab(&mut self, node: usize) {
        self.grabbed = Some((node as u32, self.disp[node]));
        self.wake_around(node);
    }

    /// Moves the dragged node's centre to `target`.
    pub fn drag_to(&mut self, target: Point) {
        if let Some((p, _)) = self.grabbed {
            self.grabbed = Some((p, sub(target, self.origin[p as usize])));
            self.awake = true;
        }
    }

    /// Drops the dragged node where it is and pins it there.
    pub fn release(&mut self) {
        if let Some((p, at)) = self.grabbed.take() {
            let p = p as usize;
            self.pinned[p] = Some(at);
            self.disp[p] = at;
            self.target[p] = at;
            self.vel[p] = Point::default();
            self.awake = true;
        }
    }

    /// Pinned nodes and their offsets from the layout.
    pub fn pins(&self) -> impl Iterator<Item = (usize, Point)> + '_ {
        self.pinned[..self.node_count]
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.map(|d| (i, d)))
    }

    /// Pins `node` at `offset` from its layout position and lets the net settle around it.
    pub fn pin(&mut self, node: usize, offset: Point) {
        self.pinned[node] = Some(offset);
        self.disp[node] = offset;
        self.target[node] = offset;
        self.wake_around(node);
    }

    /// Lets a pinned node spring back to its layout position.
    pub fn unpin(&mut self, node: usize) {
        if self.pinned[node].take().is_some() {
            self.wake_around(node);
        }
    }

    /// Unpins everything; the whole net springs back to the layout.
    pub fn reset(&mut self) {
        for node in 0..self.node_count {
            let moved = self.pinned[node].is_some() || self.disp[node] != Point::default();
            self.pinned[node] = None;
            if moved {
                self.wake_around(node);
            }
        }
    }

    fn activate(&mut self, p: usize) {
        if !self.is_active[p] {
            self.is_active[p] = true;
            self.active.push(p as u32);
        }
    }

    /// Wakes the particles nearest to `p` (breadth-first over springs), up to the budget.
    fn wake_around(&mut self, p: usize) {
        self.awake = true;
        let budget = self.active.len() + ACTIVE_BUDGET;
        let mut queue = VecDeque::from([p]);
        let mut seen = HashSet::from([p]);
        while let Some(q) = queue.pop_front() {
            if self.active.len() >= budget {
                break;
            }
            self.activate(q);
            for &s in &self.adjacent[q] {
                let s = self.springs[s as usize];
                let other = if s.a as usize == q { s.b } else { s.a } as usize;
                if seen.insert(other) {
                    queue.push_back(other);
                }
            }
        }
    }

    fn deactivate_all(&mut self) {
        for &p in &self.active {
            let p = p as usize;
            self.is_active[p] = false;
            self.vel[p] = Point::default();
            self.disp[p] = self.target[p];
        }
        self.active.clear();
    }

    /// Where a particle is held (as a displacement), if it is dragged or pinned.
    fn fixed_at(&self, i: usize) -> Option<Point> {
        match self.grabbed {
            Some((g, at)) if g as usize == i => Some(at),
            _ => self.pinned[i],
        }
    }

    /// Advances the simulation by `dt` seconds. Returns true while anything is still moving.
    pub fn step(&mut self, dt: f32, params: &NetParams) -> bool {
        if !self.is_awake() {
            return false;
        }
        let dt = dt.clamp(1.0 / 240.0, 1.0 / 30.0);
        let lambda = 0.3 + 0.6 * params.reach.clamp(0.0, 1.0);
        let k_anchor = (1.0 - lambda) * (1.0 - lambda) / lambda;
        let (zeta, use_weft, rigid) = match params.model {
            DragModel::Net => (1.0 - 0.75 * params.wobble.clamp(0.0, 1.0), true, false),
            DragModel::Strings => (1.0, false, false),
            DragModel::Rigid => (1.0, false, true),
        };

        // 1. Relax the target shape. Particles that are not woken keep their displacement.
        let mut target = std::mem::take(&mut self.target);
        for sweep in 0..SWEEPS {
            for &i in &self.active {
                let i = i as usize;
                if let Some(fixed) = self.fixed_at(i) {
                    target[i] = fixed;
                    continue;
                }
                // Bend points hold on to the layout less, so edges bend before nodes move.
                let k_home = if i < self.node_count {
                    k_anchor
                } else {
                    k_anchor * 0.5
                };
                let mut num = Point::default();
                let mut den = k_home;
                if !rigid {
                    for &si in &self.adjacent[i] {
                        let s = &self.springs[si as usize];
                        let (a, b) = (s.a as usize, s.b as usize);
                        if !s.along_edge {
                            // Between neighbours in a layer: only resist being pushed together.
                            let squeezed = dot(sub(target[b], target[a]), s.offset) < 0.0;
                            if !use_weft || !squeezed {
                                continue;
                            }
                        }
                        let other = if i == a { target[b] } else { target[a] };
                        num = add(num, scale(other, s.stiffness));
                        den += s.stiffness;
                    }
                }
                target[i] = scale(num, 1.0 / den);
            }
            // With nothing held, the net is just returning to the (overlap-free) layout.
            let holding = self.grabbed.is_some() || self.any_pinned();
            if params.avoid_overlap && holding && sweep % 4 == 3 {
                self.separate_nodes(&mut target);
            }
        }

        // 2. Move towards the targets (damped springs, semi-implicit Euler).
        let omega = std::f32::consts::TAU * FOLLOW_HZ;
        let h = dt / SUBSTEPS as f32;
        let mut max_speed = 0.0f32;
        let mut max_distance = 0.0f32;
        for &i in &self.active {
            let i = i as usize;
            if self.fixed_at(i).is_some() {
                self.disp[i] = target[i];
                self.vel[i] = Point::default();
                continue;
            }
            for _ in 0..SUBSTEPS {
                let pull = scale(sub(target[i], self.disp[i]), omega * omega);
                let drag = scale(self.vel[i], 2.0 * zeta * omega);
                self.vel[i] = add(self.vel[i], scale(sub(pull, drag), h));
                self.disp[i] = add(self.disp[i], scale(self.vel[i], h));
            }
            max_speed = max_speed.max(len(self.vel[i]));
            max_distance = max_distance.max(len(sub(target[i], self.disp[i])));
        }
        self.target = target;
        self.awake = max_speed > SLEEP_SPEED || max_distance > SLEEP_DISTANCE;
        if !self.awake && self.grabbed.is_none() {
            self.deactivate_all();
        }
        self.is_awake()
    }

    /// Pushes overlapping node boxes apart along their axis of least overlap (in `target`
    /// displacements). Only nodes near the woken ones are considered; a resting node that
    /// gets hit is woken up.
    fn separate_nodes(&mut self, target: &mut [Point]) {
        let n = self.node_count;
        let mut area: Option<(Point, Point)> = None;
        for &i in &self.active {
            let i = i as usize;
            if i < n {
                let p = add(self.origin[i], target[i]);
                let (lo, hi) = (sub(p, self.half[i]), add(p, self.half[i]));
                area = Some(match area {
                    None => (lo, hi),
                    Some((a, b)) => (
                        Point::new(a.x.min(lo.x), a.y.min(lo.y)),
                        Point::new(b.x.max(hi.x), b.y.max(hi.y)),
                    ),
                });
            }
        }
        let Some((lo, hi)) = area else { return };
        let reach = self.grid.cell.max(64.0);
        let mut near = Vec::new();
        let mut positions = HashMap::new();
        for (i, (&o, &d)) in self.origin.iter().zip(target.iter()).take(n).enumerate() {
            let (p, h) = (add(o, d), self.half[i]);
            if p.x + h.x >= lo.x - reach
                && p.x - h.x <= hi.x + reach
                && p.y + h.y >= lo.y - reach
                && p.y - h.y <= hi.y + reach
            {
                near.push(i as u32);
                positions.insert(i as u32, p);
            }
        }
        self.grid.rebuild(&near, &positions, &self.half);
        let mut pairs = Vec::new();
        self.grid.candidate_pairs(&mut pairs);
        for (a, b) in pairs {
            let (a, b) = (a as usize, b as usize);
            if !self.is_active[a] && !self.is_active[b] {
                continue;
            }
            let d = sub(
                add(self.origin[b], target[b]),
                add(self.origin[a], target[a]),
            );
            let ox = self.half[a].x + self.half[b].x + OVERLAP_MARGIN - d.x.abs();
            let oy = self.half[a].y + self.half[b].y + OVERLAP_MARGIN - d.y.abs();
            if ox <= 0.0 || oy <= 0.0 {
                continue;
            }
            self.activate(a);
            self.activate(b);
            let wa = if self.fixed_at(a).is_some() { 0.0 } else { 1.0 };
            let wb = if self.fixed_at(b).is_some() { 0.0 } else { 1.0 };
            if wa + wb == 0.0 {
                continue;
            }
            let push = if self.node_layer[a] == self.node_layer[b] {
                // Same layer: push apart along the layer, in their layout order, so that
                // neighbours never get pushed past each other.
                let along = if self.vertical {
                    Point::new(1.0, 0.0)
                } else {
                    Point::new(0.0, 1.0)
                };
                let order = if dot(sub(self.origin[b], self.origin[a]), along) < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                let needed = dot(add(self.half[a], self.half[b]), along) + OVERLAP_MARGIN;
                let have = dot(d, along) * order;
                scale(along, order * (needed - have).max(0.0))
            } else if ox < oy {
                Point::new(if d.x < 0.0 { -ox } else { ox }, 0.0)
            } else {
                Point::new(0.0, if d.y < 0.0 { -oy } else { oy })
            };
            target[a] = sub(target[a], scale(push, wa / (wa + wb)));
            target[b] = add(target[b], scale(push, wb / (wa + wb)));
        }
    }
}

/// Uniform grid for finding overlapping node boxes.
#[derive(Clone, Debug, Default)]
struct Grid {
    cell: f32,
    cells: HashMap<(i32, i32), Vec<u32>>,
}

impl Grid {
    /// Indexes the boxes of `items`, whose positions are in `pos`.
    fn rebuild(&mut self, items: &[u32], pos: &HashMap<u32, Point>, half: &[Point]) {
        self.cells.clear();
        let max_half = items.iter().fold(0.0f32, |m, &i| {
            m.max(half[i as usize].x).max(half[i as usize].y)
        });
        self.cell = (2.0 * max_half + OVERLAP_MARGIN).max(32.0);
        for &i in items {
            let (p, h) = (pos[&i], half[i as usize]);
            let (x0, y0) = self.key(p.x - h.x, p.y - h.y);
            let (x1, y1) = self.key(p.x + h.x, p.y + h.y);
            for x in x0..=x1 {
                for y in y0..=y1 {
                    self.cells.entry((x, y)).or_default().push(i);
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
    dot(a, a).sqrt()
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
                "{model:?}: pull decays: {:?}",
                (0..5).map(moved).collect::<Vec<_>>()
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
    fn only_the_neighbourhood_of_a_grab_is_simulated() {
        // A chain far longer than the active budget.
        let n = ACTIVE_BUDGET as u32 * 2;
        let input = LayoutInput {
            sizes: vec![Point::new(20.0, 10.0); n as usize],
            times: (0..n as i64).rev().collect(),
            edges: (0..n - 1)
                .map(|i| LayoutEdge {
                    child: i,
                    parent: i + 1,
                    first_parent: true,
                })
                .collect(),
            priority: Vec::new(),
        };
        let l = layout::layout(&input, &LayoutOptions::default());
        let mut net = Net::new(&l, &input.sizes);
        net.grab(0);
        assert!(net.active_count() <= ACTIVE_BUDGET + 1);
        net.drag_to(Point::new(l.nodes[0].x + 100.0, l.nodes[0].y));
        net.step(1.0 / 60.0, &NetParams::default());
        assert_eq!(
            net.node_pos(n as usize - 1),
            l.nodes[n as usize - 1],
            "far end untouched"
        );
        net.release();
        settle(&mut net, &NetParams::default());
        assert_eq!(net.active_count(), 0, "everything goes back to sleep");
    }

    #[test]
    fn pins_round_trip() {
        let (l, mut net) = chain_net();
        net.pin(3, Point::new(40.0, 0.0));
        settle(&mut net, &NetParams::default());
        assert_eq!(net.pins().collect::<Vec<_>>(), [(3, Point::new(40.0, 0.0))]);
        assert_eq!(
            net.node_pos(3),
            Point::new(l.nodes[3].x + 40.0, l.nodes[3].y)
        );
        assert!(
            net.node_pos(2).x > l.nodes[2].x,
            "neighbours follow a restored pin"
        );
    }

    #[test]
    fn neighbours_in_a_layer_keep_their_order() {
        // Root 0 with three tips side by side in one layer.
        let input = LayoutInput {
            sizes: vec![Point::new(60.0, 20.0); 4],
            times: vec![1, 4, 3, 2],
            edges: (1..4)
                .map(|t| LayoutEdge {
                    child: t,
                    parent: 0,
                    first_parent: true,
                })
                .collect(),
            priority: Vec::new(),
        };
        let l = layout::layout(&input, &LayoutOptions::default());
        let mut net = Net::new(&l, &input.sizes);
        let params = NetParams {
            model: DragModel::Strings,
            ..NetParams::default()
        };
        let order = |net: &Net| {
            let mut tips = vec![1, 2, 3];
            tips.sort_by(|&a, &b| net.node_pos(a).x.total_cmp(&net.node_pos(b).x));
            tips
        };
        let before = order(&net);
        // Drag the leftmost tip far past the others and drop it there, then reset.
        let left = before[0];
        net.grab(left);
        net.drag_to(Point::new(l.nodes[before[2]].x + 200.0, l.nodes[left].y));
        for _ in 0..60 {
            net.step(1.0 / 60.0, &params);
        }
        net.release();
        settle(&mut net, &params);
        net.reset();
        settle(&mut net, &params);
        assert_eq!(order(&net), before);
        for i in 0..4 {
            let p = net.node_pos(i);
            assert!(
                (p.x - l.nodes[i].x).abs() < 1.0 && (p.y - l.nodes[i].y).abs() < 1.0,
                "node {i} back home"
            );
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
        net.reset();
        settle(&mut net, &params);
        for i in 0..5 {
            let p = net.node_pos(i);
            assert!((p.x - l.nodes[i].x).abs() < 1.0 && (p.y - l.nodes[i].y).abs() < 1.0);
        }
    }
}
