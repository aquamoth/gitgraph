//! Moving nodes around by hand: the graph as a net of weak springs and weakly repelling nodes.
//!
//! Every node and every edge bend point is a particle with a *rest position*, at first its
//! place in the layout. Springs along every edge keep the offsets between rest positions, so a
//! dragged node pulls its neighbours along. Nodes push each other away when they come closer
//! than they rest (up to a short range), like weak magnets; nodes side by side in a row keep
//! their order, like beads on a string, unless one is lifted out of the row. Edges are pushed
//! aside the same way by their neighbours in a layer. Each particle is weakly anchored to its
//! rest position, which limits how far a pull spreads.
//!
//! What a drag does depends on the [`DragModel`]. In [`DragModel::Adapt`] the rest of the graph
//! gives way a little; in the other models only the dragged nodes move and the edges to them
//! stretch. Dropping makes the new shape permanent: every particle that moved now rests where
//! it is, so the springs keep the *new* offsets and nothing drifts back. Dropped nodes are not
//! pinned; they give way to later drags like any other node. Every drop can be undone, and a
//! reset sends everything back to the layout.
//!
//! The state is kept as *displacements from the layout*, which stay small and therefore exact
//! even where layout coordinates run into the millions. Each frame of an adaptive drag has two
//! parts:
//!
//! 1. **Shape.** The net's shape for the current drag minimises
//!    `Σ k_s |d_b − d_a − (h_b − h_a)|² + Σ k_a |d_i − h_i|²` (plus the magnets) over
//!    displacements `d`, where `h` are the rest displacements and the dragged nodes are held
//!    fixed. It is found by Gauss-Seidel relaxation over the woken particles, warm-started from
//!    the previous frame. In a chain the displacement decays by a factor λ per hop when
//!    `k_a / k_s = (1 − λ)² / λ`, which is what [`NetParams::pull`] sets.
//! 2. **Motion.** Every particle follows its target through a damped spring, so the net moves
//!    with some inertia and, depending on [`NetParams::wobble`], overshoots a little.
//!
//! Only particles near the dragged nodes are simulated (breadth-first up to a budget), so
//! dragging stays smooth in graphs with a million bend points.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::layout::{Layout, Point};

/// What moves when nodes are dragged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DragModel {
    /// The dragged nodes follow the pointer and the rest of the graph gives way a little:
    /// neighbours are pulled along their edges, and nodes in the way are pushed aside.
    #[default]
    #[serde(alias = "Net", alias = "Strings")]
    Adapt,
    /// Only the dragged nodes move; the edges to them stretch.
    #[serde(alias = "Rigid")]
    Free,
    /// The dragged nodes move together with everything that grows out of them
    /// ([`crate::revgraph::RevGraph::subtree`]); nothing else moves. The caller passes the
    /// whole subtree to [`Net::grab`].
    Subtree,
}

impl DragModel {
    pub const ALL: [DragModel; 3] = [DragModel::Adapt, DragModel::Free, DragModel::Subtree];

    pub fn label(self) -> &'static str {
        match self {
            DragModel::Adapt => "Adapt",
            DragModel::Free => "Free",
            DragModel::Subtree => "Subtree",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            DragModel::Adapt => {
                "The graph gives way: neighbours follow, nodes in the way move aside"
            }
            DragModel::Free => "Move only the selected nodes; nothing else moves",
            DragModel::Subtree => {
                "Move the selected nodes and everything that grows out of them \
                 (their first-parent descendants); nothing else moves"
            }
        }
    }

    /// True if the rest of the graph reacts to a drag.
    pub fn adapts(self) -> bool {
        self == DragModel::Adapt
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetParams {
    pub model: DragModel,
    /// 0..=1: how far a pull spreads along the edges (Adapt).
    pub pull: f32,
    /// 0..=1: how strongly, and from how far, nodes push each other away (Adapt).
    pub push: f32,
    /// 0..=1: how much the net overshoots and wobbles before settling (Adapt).
    pub wobble: f32,
    /// Push overlapping nodes apart (Adapt).
    pub avoid_overlap: bool,
}

impl Default for NetParams {
    fn default() -> Self {
        NetParams {
            model: DragModel::Adapt,
            pull: 0.3,
            push: 0.5,
            wobble: 0.4,
            avoid_overlap: true,
        }
    }
}

/// At most this many particles are woken around the grabbed nodes; the rest of a huge graph
/// stays still (a pull has decayed to nothing long before that many hops).
const ACTIVE_BUDGET: usize = 8_000;
/// Particles woken around a node that gets pushed from outside the simulated neighbourhood.
const PUSH_WAKE: usize = 32;
/// Gauss-Seidel sweeps over the woken particles per frame.
const SWEEPS: usize = 12;
/// Sweeps when a drag ends, so that the shape that becomes permanent has converged.
const FINAL_SWEEPS: usize = 48;
/// Integration substeps per frame for following the targets.
const SUBSTEPS: usize = 4;
/// Natural frequency (Hz) with which particles follow their targets.
const FOLLOW_HZ: f32 = 3.0;
/// Minimum gap kept between node boxes when avoiding overlap.
const OVERLAP_MARGIN: f32 = 6.0;
/// At `push` = 1, nodes closer than this (gap between boxes, layout units) push each other
/// away, with this stiffness (edge springs have 1). Both scale linearly with `push`.
const MAGNET_RANGE: f32 = 64.0;
const MAGNET_STIFFNESS: f32 = 1.5;
/// Nearby nodes are collected once per frame, up to this far beyond the magnet range.
const NEAR_SLACK: f32 = 24.0;
/// A drop changes a particle's rest position only if it moved more than this.
const REST_EPSILON: f32 = 0.05;
/// Number of drops kept for undo, and how many particle changes they may hold in all (about
/// 20 bytes each).
const UNDO_LIMIT: usize = 200;
const UNDO_ENTRIES: usize = 1_000_000;
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

/// A drag in progress.
#[derive(Clone, Debug)]
struct Grab {
    /// The node under the pointer, and its displacement when grabbed.
    anchor: u32,
    anchor_start: Point,
    /// Held particles and their displacements when grabbed; they all move by `delta`.
    held: Vec<(u32, Point)>,
    /// Nodes that count as moved by hand once dropped.
    marked: Vec<u32>,
    /// Edges with a held end and bend points that are not all held: they stretch.
    stretched: Vec<u32>,
    /// Bend points of the stretched edges (which follow them exactly unless the net adapts).
    stretched_bends: Vec<u32>,
    delta: Point,
    /// True if the rest of the net gives way; false if it stays still.
    adapt: bool,
}

/// One undoable step: rest positions and "moved by hand" marks before and after.
#[derive(Clone, Debug, Default)]
struct Change {
    homes: Vec<(u32, Point, Point)>,
    marks: Vec<(u32, bool, bool)>,
}

impl Change {
    fn is_empty(&self) -> bool {
        self.homes.is_empty() && self.marks.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct Net {
    node_count: usize,
    /// Layout position of every particle.
    origin: Vec<Point>,
    /// Rest position of every particle, as a displacement from the layout.
    home: Vec<Point>,
    /// Number of particles that rest away from the layout.
    displaced: usize,
    /// Current displacement from the layout, its velocity, and where it is heading.
    disp: Vec<Point>,
    vel: Vec<Point>,
    target: Vec<Point>,
    /// Half extents of node boxes (zero for bend points).
    half: Vec<Point>,
    /// Where every woken particle was heading at the start of the frame.
    before: Vec<Point>,
    /// Whether layers run horizontally (newest on top or bottom).
    vertical: bool,
    /// Nodes the user moved by hand.
    moved: Vec<bool>,
    springs: Vec<Spring>,
    /// Spring ids per particle.
    adjacent: Vec<Vec<u32>>,
    /// Particles of every edge, child node first, parent node last.
    chains: Vec<Vec<u32>>,
    /// Edges at every node.
    node_edges: Vec<Vec<u32>>,
    /// Number of edges through every bend point (more than one where edges are bundled),
    /// indexed by particle minus `node_count`.
    bend_edges: Vec<u32>,
    grab: Option<Grab>,
    /// Particles held by the current grab.
    held: Vec<bool>,
    awake: bool,
    /// Particles follow their targets with some overshoot (during and after an adaptive drag)
    /// or critically damped.
    wobbly: bool,
    /// Particles being simulated; everything else is at rest.
    active: Vec<u32>,
    is_active: Vec<bool>,
    grid: Grid,
    /// Per frame: nodes near each node that push it away, and the gap they keep.
    magnets: Vec<Vec<(u32, f32)>>,
    magnet_nodes: Vec<u32>,
    undo: Vec<Change>,
    redo: Vec<Change>,
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
        let mut node_edges = vec![Vec::new(); n];
        let mut bend_edges: Vec<u32> = Vec::new();
        let mut springs = Vec::new();
        let mut seen_segments = HashSet::new();
        // Bend point identity -> particle, so bundled edges share their particles.
        let mut bend_particle: HashMap<u32, u32> = HashMap::new();

        for (e, pts) in layout.edges.iter().enumerate() {
            let (child, parent) = layout.edge_ends[e];
            node_edges[child as usize].push(e as u32);
            if parent != child {
                node_edges[parent as usize].push(e as u32);
            }
            let mut chain = vec![child];
            for (k, p) in pts[1..pts.len() - 1].iter().enumerate() {
                let id = layout.edge_bends.get(e).and_then(|b| b.get(k)).copied();
                let particle = match id.and_then(|id| bend_particle.get(&id)) {
                    Some(&particle) => particle,
                    None => {
                        let particle = origin.len() as u32;
                        origin.push(*p);
                        half.push(Point::default());
                        bend_edges.push(0);
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
            // Count each edge once per bend point, even if it passes it twice.
            let mut bends: Vec<u32> = chain[1..chain.len() - 1].to_vec();
            bends.sort_unstable();
            bends.dedup();
            for p in bends {
                bend_edges[p as usize - n] += 1;
            }
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
            home: vec![Point::default(); count],
            displaced: 0,
            disp: vec![Point::default(); count],
            vel: vec![Point::default(); count],
            target: vec![Point::default(); count],
            half,
            before: vec![Point::default(); count],
            vertical,
            moved: vec![false; n],
            springs,
            adjacent,
            chains,
            node_edges,
            bend_edges,
            grab: None,
            held: vec![false; count],
            awake: false,
            wobbly: false,
            active: Vec::new(),
            is_active: vec![false; count],
            grid: Grid::default(),
            magnets: vec![Vec::new(); n],
            magnet_nodes: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
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

    /// True if the user moved `node` by hand (and has not returned it to the layout).
    pub fn is_moved(&self, node: usize) -> bool {
        self.moved[node]
    }

    /// True if `node` rests somewhere else than in the layout.
    pub fn is_displaced(&self, node: usize) -> bool {
        self.home[node] != Point::default()
    }

    /// True if anything rests somewhere else than in the layout.
    pub fn any_displaced(&self) -> bool {
        self.displaced > 0
    }

    fn set_home(&mut self, p: usize, h: Point) {
        let zero = Point::default();
        match (self.home[p] == zero, h == zero) {
            (true, false) => self.displaced += 1,
            (false, true) => self.displaced -= 1,
            _ => {}
        }
        self.home[p] = h;
    }

    /// True while the simulation still has motion to show.
    pub fn is_awake(&self) -> bool {
        self.awake || self.grab.is_some()
    }

    /// The node under the pointer while dragging.
    pub fn grabbed(&self) -> Option<usize> {
        self.grab.as_ref().map(|g| g.anchor as usize)
    }

    /// Number of particles currently simulated.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Starts dragging `nodes` (usually including `anchor`, the node under the pointer), and
    /// with them the `carried` nodes. They all move rigidly together; when dropped, `nodes` and
    /// `anchor` count as moved by hand. If `adapt` is true the rest of the net gives way;
    /// otherwise it stays still and only the edges to the dragged nodes stretch.
    pub fn grab(&mut self, anchor: usize, nodes: &[usize], carried: &[usize], adapt: bool) {
        self.cancel_grab();
        let n = self.node_count;
        let mut marked: Vec<u32> = nodes
            .iter()
            .chain([&anchor])
            .filter(|&&node| node < n)
            .map(|&node| node as u32)
            .collect();
        marked.sort_unstable();
        marked.dedup();
        let mut held: Vec<u32> = Vec::new();
        let carried = carried.iter().filter(|&&c| c < n).map(|&c| c as u32);
        for node in marked.iter().copied().chain(carried) {
            if !self.held[node as usize] {
                self.held[node as usize] = true;
                held.push(node);
            }
        }
        // Edges at held nodes. Bend points whose edges all run between held nodes are held too.
        let mut edges: Vec<u32> = held
            .iter()
            .flat_map(|&node| self.node_edges[node as usize].iter().copied())
            .collect();
        edges.sort_unstable();
        edges.dedup();
        let mut internal: HashMap<u32, u32> = HashMap::new();
        for &e in &edges {
            let chain = &self.chains[e as usize];
            let (a, b) = (chain[0] as usize, chain[chain.len() - 1] as usize);
            if self.held[a] && self.held[b] {
                let mut bends: Vec<u32> = chain[1..chain.len() - 1].to_vec();
                bends.sort_unstable();
                bends.dedup();
                for p in bends {
                    *internal.entry(p).or_default() += 1;
                }
            }
        }
        for (p, k) in internal {
            if k == self.bend_edges[p as usize - n] {
                self.held[p as usize] = true;
                held.push(p);
            }
        }
        let stretched: Vec<u32> = edges
            .into_iter()
            .filter(|&e| {
                let chain = &self.chains[e as usize];
                chain[1..chain.len() - 1]
                    .iter()
                    .any(|&p| !self.held[p as usize])
            })
            .collect();
        let mut stretched_bends: Vec<u32> = stretched
            .iter()
            .flat_map(|&e| {
                let chain = &self.chains[e as usize];
                chain[1..chain.len() - 1].iter().copied()
            })
            .filter(|&p| !self.held[p as usize])
            .collect();
        stretched_bends.sort_unstable();
        stretched_bends.dedup();

        for &p in &held {
            self.activate(p as usize);
        }
        if adapt {
            self.wake_around(&held, ACTIVE_BUDGET);
        } else {
            for &p in &stretched_bends {
                self.activate(p as usize);
            }
        }
        self.wobbly = adapt;
        self.awake = true;
        self.grab = Some(Grab {
            anchor: anchor as u32,
            anchor_start: self.disp[anchor],
            held: held.iter().map(|&p| (p, self.disp[p as usize])).collect(),
            marked,
            stretched,
            stretched_bends,
            delta: Point::default(),
            adapt,
        });
    }

    /// Moves the node under the pointer's centre to `target`; the other dragged nodes keep
    /// their offsets from it.
    pub fn drag_to(&mut self, target: Point) {
        if let Some(g) = &mut self.grab {
            let at = sub(target, self.origin[g.anchor as usize]);
            g.delta = sub(at, g.anchor_start);
            self.awake = true;
        }
    }

    /// Drops the dragged nodes where they are. The shape the net has taken becomes its new
    /// resting shape, and the dragged nodes are marked as moved. Undoable.
    pub fn release(&mut self, params: &NetParams) {
        match &self.grab {
            None => return,
            // Let go without moving: nothing changes.
            Some(g) if len(g.delta) <= REST_EPSILON => {
                self.cancel_grab();
                return;
            }
            Some(_) => {}
        }
        self.shape(params, FINAL_SWEEPS);
        let grab = self.grab.take().expect("grab exists");
        let mut change = Change::default();
        for k in 0..self.active.len() {
            let p = self.active[k] as usize;
            let t = self.target[p];
            if len(sub(t, self.home[p])) > REST_EPSILON {
                change.homes.push((p as u32, self.home[p], t));
                self.set_home(p, t);
            }
            self.target[p] = self.home[p];
        }
        for &(p, _) in &grab.held {
            self.held[p as usize] = false;
        }
        for &node in &grab.marked {
            if !self.moved[node as usize] {
                self.moved[node as usize] = true;
                change.marks.push((node, false, true));
            }
        }
        self.record(change);
        self.awake = true;
    }

    /// Ends a drag without keeping anything from it.
    fn cancel_grab(&mut self) {
        if let Some(grab) = self.grab.take() {
            for &(p, _) in &grab.held {
                self.held[p as usize] = false;
            }
            self.awake = true;
        }
    }

    fn record(&mut self, change: Change) {
        if change.is_empty() {
            return;
        }
        self.undo.push(change);
        self.redo.clear();
        // Forget the oldest steps beyond the limits (but always keep the last one).
        let size = |c: &Change| c.homes.len() + c.marks.len();
        let mut total: usize = self.undo.iter().map(size).sum();
        let mut drop = 0;
        while self.undo.len() - drop > 1
            && (self.undo.len() - drop > UNDO_LIMIT || total > UNDO_ENTRIES)
        {
            total -= size(&self.undo[drop]);
            drop += 1;
        }
        self.undo.drain(..drop);
    }

    /// Applies a change (or reverts it); the particles concerned move there smoothly.
    fn apply(&mut self, change: &Change, forward: bool) {
        for &(p, before, after) in &change.homes {
            self.set_home(p as usize, if forward { after } else { before });
            self.activate(p as usize);
        }
        for &(node, before, after) in &change.marks {
            self.moved[node as usize] = if forward { after } else { before };
        }
        self.wobbly = false;
        self.awake = true;
    }

    /// Reverts the last drop, reset or return to the layout (ending any drag). Returns false if
    /// there is none.
    pub fn undo(&mut self) -> bool {
        let Some(change) = self.undo.pop() else {
            return false;
        };
        self.cancel_grab();
        self.apply(&change, false);
        self.redo.push(change);
        true
    }

    /// Repeats the last undone change (ending any drag). Returns false if there is none.
    pub fn redo(&mut self) -> bool {
        let Some(change) = self.redo.pop() else {
            return false;
        };
        self.cancel_grab();
        self.apply(&change, true);
        self.undo.push(change);
        true
    }

    /// Sends everything back to the layout. Undoable.
    pub fn reset(&mut self) {
        self.cancel_grab();
        let mut change = Change::default();
        for (p, &h) in self.home.iter().enumerate() {
            if h != Point::default() {
                change.homes.push((p as u32, h, Point::default()));
            }
        }
        for (node, &moved) in self.moved.iter().enumerate() {
            if moved {
                change.marks.push((node as u32, true, false));
            }
        }
        self.apply(&change, true);
        self.record(change);
    }

    /// Sends `nodes` back to their layout positions; nothing else moves, the edges at them
    /// stretch. Undoable.
    pub fn return_to_layout(&mut self, nodes: &[usize]) {
        self.cancel_grab();
        let by: HashMap<u32, Point> = nodes
            .iter()
            .filter(|&&node| node < self.node_count)
            .map(|&node| (node as u32, scale(self.home[node], -1.0)))
            .collect();
        let mut change = Change {
            homes: self.shifted_homes(&by),
            marks: Vec::new(),
        };
        for &node in by.keys() {
            if self.moved[node as usize] {
                change.marks.push((node, true, false));
            }
        }
        self.apply(&change, true);
        self.record(change);
    }

    /// Nodes that rest away from the layout or were moved by hand: node, rest offset from the
    /// layout, moved by hand. For saving; see [`Net::restore`].
    pub fn rest_offsets(&self) -> impl Iterator<Item = (usize, Point, bool)> + '_ {
        (0..self.node_count)
            .filter(|&i| self.moved[i] || self.home[i] != Point::default())
            .map(|i| (i, self.home[i], self.moved[i]))
    }

    /// Puts nodes at saved rest offsets from the layout at once (the edges at them follow).
    /// Meant for a fresh net; not recorded for undo.
    pub fn restore(&mut self, saved: impl IntoIterator<Item = (usize, Point, bool)>) {
        self.cancel_grab();
        let mut by = HashMap::new();
        for (node, offset, moved) in saved {
            if node < self.node_count && offset.x.is_finite() && offset.y.is_finite() {
                by.insert(node as u32, sub(offset, self.home[node]));
                self.moved[node] = moved;
            }
        }
        for (p, _, after) in self.shifted_homes(&by) {
            let p = p as usize;
            self.set_home(p, after);
            self.disp[p] = after;
            self.target[p] = after;
            self.vel[p] = Point::default();
        }
    }

    /// Rest positions after moving the given nodes by the given amounts, with the bend points
    /// of their edges moved proportionally: (particle, before, after).
    fn shifted_homes(&self, by: &HashMap<u32, Point>) -> Vec<(u32, Point, Point)> {
        let mut out: Vec<(u32, Point, Point)> = by
            .iter()
            .filter(|&(_, &d)| d != Point::default())
            .map(|(&node, &d)| {
                let h = self.home[node as usize];
                (node, h, add(h, d))
            })
            .collect();
        let mut edges: Vec<u32> = by
            .keys()
            .flat_map(|&node| self.node_edges[node as usize].iter().copied())
            .collect();
        edges.sort_unstable();
        edges.dedup();
        let shifts = self.bend_shifts(&edges, |node| by.get(&node).copied().unwrap_or_default());
        for (p, s) in shifts {
            let h = self.home[p as usize];
            if s != Point::default() {
                out.push((p, h, add(h, s)));
            }
        }
        out
    }

    /// How far the bend points of `edges` move when their end nodes move by `shift_of`: each
    /// edge's bend points share the movement of its ends in proportion to their distance along
    /// the edge. A bend point shared by several (bundled) edges gets the average over all of
    /// them.
    fn bend_shifts(&self, edges: &[u32], shift_of: impl Fn(u32) -> Point) -> HashMap<u32, Point> {
        let n = self.node_count;
        let mut out: HashMap<u32, Point> = HashMap::new();
        let rest = |p: u32| (self.origin[p as usize], self.home[p as usize]);
        let mut run = Vec::new();
        for &e in edges {
            let chain = &self.chains[e as usize];
            let last = chain.len() - 1;
            let (sa, sb) = (shift_of(chain[0]), shift_of(chain[last]));
            if last < 2 || (sa == Point::default() && sb == Point::default()) {
                continue;
            }
            // Distance along the edge's resting shape.
            run.clear();
            run.push(0.0f32);
            for w in chain.windows(2) {
                let ((oa, ha), (ob, hb)) = (rest(w[0]), rest(w[1]));
                let step = len(add(sub(ob, oa), sub(hb, ha)));
                run.push(run[run.len() - 1] + step);
            }
            let total = run[last];
            for (k, &p) in chain.iter().enumerate().take(last).skip(1) {
                let t = if total > 0.0 {
                    run[k] / total
                } else {
                    k as f32 / last as f32
                };
                let s = add(scale(sa, 1.0 - t), scale(sb, t));
                let share = scale(s, 1.0 / self.bend_edges[p as usize - n] as f32);
                let slot = out.entry(p).or_default();
                *slot = add(*slot, share);
            }
        }
        out
    }

    fn activate(&mut self, p: usize) {
        let t = self.target[p];
        self.activate_heading(p, t);
    }

    /// Wakes particle `p`, which is heading for `target` (for use while `self.target` is
    /// being relaxed elsewhere).
    fn activate_heading(&mut self, p: usize, target: Point) {
        if !self.is_active[p] {
            self.is_active[p] = true;
            self.before[p] = target;
            self.active.push(p as u32);
        }
    }

    /// Wakes the particles nearest to `from` (breadth-first over springs), up to `budget`
    /// more than are awake already.
    fn wake_around(&mut self, from: &[u32], budget: usize) {
        self.awake = true;
        let limit = self.active.len() + budget;
        let mut queue: VecDeque<usize> = from.iter().map(|&p| p as usize).collect();
        let mut seen: HashSet<usize> = queue.iter().copied().collect();
        while let Some(q) = queue.pop_front() {
            if self.active.len() >= limit {
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

    /// Advances the simulation by `dt` seconds. Returns true while anything is still moving.
    pub fn step(&mut self, dt: f32, params: &NetParams) -> bool {
        if !self.is_awake() {
            return false;
        }
        let dt = dt.clamp(1.0 / 240.0, 1.0 / 30.0);
        self.shape(params, SWEEPS);

        // Move towards the targets (damped springs, semi-implicit Euler). Held particles, and
        // the stretched edges of a drag that does not adapt, follow exactly.
        if let Some(g) = &self.grab
            && !g.adapt
        {
            for &p in &g.stretched_bends {
                self.disp[p as usize] = self.target[p as usize];
                self.vel[p as usize] = Point::default();
            }
        }
        let zeta = if self.wobbly {
            1.0 - 0.75 * params.wobble.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let omega = std::f32::consts::TAU * FOLLOW_HZ;
        let h = dt / SUBSTEPS as f32;
        let mut max_speed = 0.0f32;
        let mut max_distance = 0.0f32;
        for &i in &self.active {
            let i = i as usize;
            if self.held[i] {
                self.disp[i] = self.target[i];
                self.vel[i] = Point::default();
                continue;
            }
            for _ in 0..SUBSTEPS {
                let pull = scale(sub(self.target[i], self.disp[i]), omega * omega);
                let drag = scale(self.vel[i], 2.0 * zeta * omega);
                self.vel[i] = add(self.vel[i], scale(sub(pull, drag), h));
                self.disp[i] = add(self.disp[i], scale(self.vel[i], h));
            }
            max_speed = max_speed.max(len(self.vel[i]));
            max_distance = max_distance.max(len(sub(self.target[i], self.disp[i])));
        }
        self.awake = max_speed > SLEEP_SPEED || max_distance > SLEEP_DISTANCE;
        if !self.awake && self.grab.is_none() {
            self.deactivate_all();
        }
        self.is_awake()
    }

    /// Computes where every woken particle is heading.
    fn shape(&mut self, params: &NetParams, sweeps: usize) {
        let Some(grab) = &self.grab else {
            for &i in &self.active {
                self.target[i as usize] = self.home[i as usize];
            }
            return;
        };
        if !grab.adapt {
            // Everything else stays (or comes to rest) where it rests; the edges at the dragged
            // nodes stretch evenly.
            for &i in &self.active {
                let i = i as usize;
                if !self.held[i] {
                    self.target[i] = self.home[i];
                }
            }
            for &(p, start) in &grab.held {
                self.target[p as usize] = add(start, grab.delta);
            }
            let shifts = self.bend_shifts(&grab.stretched, |node| {
                sub(self.target[node as usize], self.home[node as usize])
            });
            for (p, s) in shifts {
                if !self.held[p as usize] {
                    self.target[p as usize] = add(self.home[p as usize], s);
                }
            }
            return;
        }

        // Where everything was heading at the start of the frame decides on which side of each
        // other things stay.
        for &i in &self.active {
            self.before[i as usize] = self.target[i as usize];
        }
        for &(p, start) in &grab.held {
            self.target[p as usize] = add(start, grab.delta);
        }
        let lambda = 0.3 + 0.6 * params.pull.clamp(0.0, 1.0);
        let k_anchor = (1.0 - lambda) * (1.0 - lambda) / lambda;
        let push = params.push.clamp(0.0, 1.0);
        let (range, k_magnet) = (MAGNET_RANGE * push, MAGNET_STIFFNESS * push);
        let weft_reach = range + OVERLAP_MARGIN;
        let near = if push > 0.0 || params.avoid_overlap {
            self.near_pairs(range + NEAR_SLACK)
        } else {
            Vec::new()
        };
        self.collect_magnets(&near, range, k_magnet > 0.0);

        let n = self.node_count;
        let mut target = std::mem::take(&mut self.target);
        for sweep in 0..sweeps {
            for &i in &self.active {
                let i = i as usize;
                if self.held[i] {
                    continue;
                }
                // Bend points hold on to their rest less, so edges bend before nodes move.
                let k_home = if i < n { k_anchor } else { k_anchor * 0.5 };
                let mut num = scale(self.home[i], k_home);
                let mut den = k_home;
                for &si in &self.adjacent[i] {
                    let s = &self.springs[si as usize];
                    let (a, b) = (s.a as usize, s.b as usize);
                    let rest = sub(self.home[b], self.home[a]);
                    if !s.along_edge {
                        // Between neighbours in a layer: keep edges from being pushed into each
                        // other or through nodes (the magnets keep nodes apart). Only resist
                        // coming closer than they rest, and only once they are near.
                        if a < n && b < n {
                            continue;
                        }
                        let apart = add(s.offset, rest);
                        let dist = len(apart);
                        if dist == 0.0 {
                            continue;
                        }
                        let mut u = scale(apart, 1.0 / dist);
                        let ext = add(self.half[a], self.half[b]);
                        let reach = u.x.abs() * ext.x + u.y.abs() * ext.y + weft_reach;
                        // Keep them on the side they were on; once something has passed, it
                        // stays passed.
                        let was = |p: usize| {
                            if self.is_active[p] {
                                self.before[p]
                            } else {
                                target[p]
                            }
                        };
                        let passed = dot(add(s.offset, sub(was(b), was(a))), u) < 0.0;
                        let keep = if passed {
                            u = scale(u, -1.0);
                            reach
                        } else {
                            dist.min(reach)
                        };
                        let now = dot(add(s.offset, sub(target[b], target[a])), u);
                        if now >= keep {
                            continue;
                        }
                        let push = scale(u, keep - now);
                        let want = if i == a {
                            sub(target[a], push)
                        } else {
                            add(target[b], push)
                        };
                        num = add(num, scale(want, s.stiffness));
                        den += s.stiffness;
                        continue;
                    }
                    let want = if i == a {
                        sub(target[b], rest)
                    } else {
                        add(target[a], rest)
                    };
                    num = add(num, scale(want, s.stiffness));
                    den += s.stiffness;
                }
                if i < n {
                    for &(k, keep) in &self.magnets[i] {
                        let pair = &near[k as usize];
                        let (gap, away) = self.gap(pair, &target, 0.0);
                        if gap < keep {
                            let away = if pair.b as usize == i {
                                away
                            } else {
                                scale(away, -1.0)
                            };
                            let want = add(target[i], scale(away, keep - gap));
                            num = add(num, scale(want, k_magnet));
                            den += k_magnet;
                        }
                    }
                }
                target[i] = scale(num, 1.0 / den);
            }
            if params.avoid_overlap && sweep % 4 == 3 {
                self.separate_nodes(&mut target, &near);
            }
        }
        self.target = target;
        for &node in &self.magnet_nodes {
            self.magnets[node as usize].clear();
        }
        self.magnet_nodes.clear();
    }

    /// Pairs of nodes whose boxes are less than `pad` apart, at least one of them away from
    /// its rest position (two resting nodes cannot press on each other), and how they sat at
    /// the start of the frame.
    fn near_pairs(&mut self, pad: f32) -> Vec<Near> {
        let n = self.node_count;
        let place = |i: usize| add(self.origin[i], self.target[i]);
        let moving =
            |i: usize| self.held[i] || len(sub(self.target[i], self.home[i])) > REST_EPSILON;
        let mut area: Option<(Point, Point)> = None;
        for &i in &self.active {
            let i = i as usize;
            if i < n && moving(i) {
                let (lo, hi) = (sub(place(i), self.half[i]), add(place(i), self.half[i]));
                area = Some(match area {
                    None => (lo, hi),
                    Some((a, b)) => (
                        Point::new(a.x.min(lo.x), a.y.min(lo.y)),
                        Point::new(b.x.max(hi.x), b.y.max(hi.y)),
                    ),
                });
            }
        }
        let Some((lo, hi)) = area else {
            return Vec::new();
        };
        let mut items = Vec::new();
        for i in 0..n {
            let (p, h) = (place(i), self.half[i]);
            if p.x + h.x >= lo.x - pad
                && p.x - h.x <= hi.x + pad
                && p.y + h.y >= lo.y - pad
                && p.y - h.y <= hi.y + pad
            {
                let grown = Point::new(h.x + pad / 2.0, h.y + pad / 2.0);
                items.push((i as u32, p, grown));
            }
        }
        self.grid.rebuild(&items);
        let mut pairs = Vec::new();
        self.grid.candidate_pairs(&mut pairs);
        let (along, across) = self.axes();
        pairs
            .into_iter()
            .filter_map(|(a, b)| {
                let (a, b) = (a as usize, b as usize);
                if !moving(a) && !moving(b) {
                    return None;
                }
                let d = add(
                    sub(self.origin[b], self.origin[a]),
                    sub(self.target[b], self.target[a]),
                );
                let ext = add(self.half[a], self.half[b]);
                if d.x.abs() >= ext.x + pad || d.y.abs() >= ext.y + pad {
                    return None;
                }
                // Side by side in a row: they keep their order along it.
                let was = |p: usize| {
                    if self.is_active[p] {
                        self.before[p]
                    } else {
                        self.target[p]
                    }
                };
                let d = add(sub(self.origin[b], self.origin[a]), sub(was(b), was(a)));
                let in_row = dot(d, across).abs() < dot(ext, across);
                let side = match (in_row, dot(d, along) < 0.0) {
                    (false, _) => 0.0,
                    (true, true) => -1.0,
                    (true, false) => 1.0,
                };
                let rest = add(
                    sub(self.origin[b], self.origin[a]),
                    sub(self.home[b], self.home[a]),
                );
                Some(Near {
                    a: a as u32,
                    b: b as u32,
                    side,
                    rest_gap: box_gap(rest, ext).0,
                })
            })
            .collect()
    }

    /// Unit vectors along the layers and across them.
    fn axes(&self) -> (Point, Point) {
        if self.vertical {
            (Point::new(1.0, 0.0), Point::new(0.0, 1.0))
        } else {
            (Point::new(0.0, 1.0), Point::new(1.0, 0.0))
        }
    }

    /// Gap between the boxes of a pair of nodes, grown by `margin`, for displacements `t`;
    /// and the unit direction in which moving `b` widens it. Nodes side by side in a row
    /// measure it along the row, in their order.
    fn gap(&self, pair: &Near, t: &[Point], margin: f32) -> (f32, Point) {
        let (a, b) = (pair.a as usize, pair.b as usize);
        let d = add(sub(self.origin[b], self.origin[a]), sub(t[b], t[a]));
        let ext = add(add(self.half[a], self.half[b]), Point::new(margin, margin));
        if pair.side != 0.0 {
            let (along, _) = self.axes();
            let gap = dot(d, along) * pair.side - dot(ext, along);
            (gap, scale(along, pair.side))
        } else {
            box_gap(d, ext)
        }
    }

    /// Sets up this frame's magnets between the nearby pairs, and wakes resting nodes that are
    /// pressed on. A pair keeps the gap it has at rest, or `range` if that is smaller.
    fn collect_magnets(&mut self, near: &[Near], range: f32, magnets: bool) {
        for (k, pair) in near.iter().enumerate() {
            let (a, b) = (pair.a as usize, pair.b as usize);
            if self.held[a] && self.held[b] {
                continue;
            }
            let keep = pair.rest_gap.min(range);
            // Wake a resting node only once something actually presses on it.
            let now = self.gap(pair, &self.target, 0.0).0;
            let pressed = now < pair.margin() || magnets && now < keep - REST_EPSILON;
            if pressed {
                for p in [a, b] {
                    if !self.is_active[p] {
                        self.wake_around(&[p as u32], PUSH_WAKE);
                    }
                }
            }
            if magnets {
                for p in [a, b] {
                    if self.magnets[p].is_empty() {
                        self.magnet_nodes.push(p as u32);
                    }
                    self.magnets[p].push((k as u32, keep));
                }
            }
        }
    }

    /// Pushes overlapping node boxes apart (in `target` displacements): nodes side by side in
    /// a row along the row, in their order, so that they never get pushed past each other;
    /// others along their axis of least overlap. A resting node that gets hit is woken up.
    fn separate_nodes(&mut self, target: &mut [Point], near: &[Near]) {
        for pair in near {
            let (gap, away) = self.gap(pair, target, pair.margin());
            if gap >= 0.0 {
                continue;
            }
            let (a, b) = (pair.a as usize, pair.b as usize);
            self.activate_heading(a, target[a]);
            self.activate_heading(b, target[b]);
            let wa = if self.held[a] { 0.0 } else { 1.0 };
            let wb = if self.held[b] { 0.0 } else { 1.0 };
            if wa + wb == 0.0 {
                continue;
            }
            let push = scale(away, -gap);
            target[a] = sub(target[a], scale(push, wa / (wa + wb)));
            target[b] = add(target[b], scale(push, wb / (wa + wb)));
        }
    }
}

/// Two nodes near each other during a frame.
#[derive(Clone, Copy, Debug)]
struct Near {
    a: u32,
    b: u32,
    /// ±1 if they sat side by side in a row at the start of the frame (`b` after `a` along the
    /// row, or before it), 0 otherwise.
    side: f32,
    /// Gap between their boxes at rest.
    rest_gap: f32,
}

impl Near {
    /// The gap that overlap avoidance keeps: the margin, or less if they rest closer.
    fn margin(&self) -> f32 {
        self.rest_gap.min(OVERLAP_MARGIN)
    }
}

/// Gap between two boxes whose centres are `d` apart and whose half extents add up to `ext`
/// (negative if they overlap), and the unit direction in which moving the second box away
/// from the first widens it fastest.
fn box_gap(d: Point, ext: Point) -> (f32, Point) {
    let (gx, gy) = (d.x.abs() - ext.x, d.y.abs() - ext.y);
    let sx = if d.x < 0.0 { -1.0 } else { 1.0 };
    let sy = if d.y < 0.0 { -1.0 } else { 1.0 };
    if gx > 0.0 && gy > 0.0 {
        let g = (gx * gx + gy * gy).sqrt();
        (g, Point::new(sx * gx / g, sy * gy / g))
    } else if gx > gy {
        (gx, Point::new(sx, 0.0))
    } else {
        (gy, Point::new(0.0, sy))
    }
}

/// Uniform grid for finding overlapping boxes.
#[derive(Clone, Debug, Default)]
struct Grid {
    cell: f32,
    cells: HashMap<(i32, i32), Vec<u32>>,
}

impl Grid {
    /// Indexes boxes given as (id, centre, half extents).
    fn rebuild(&mut self, items: &[(u32, Point, Point)]) {
        self.cells.clear();
        let max_half = items
            .iter()
            .fold(0.0f32, |m, &(_, _, h)| m.max(h.x).max(h.y));
        self.cell = (2.0 * max_half).max(32.0);
        for &(i, p, h) in items {
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

    fn net_for(input: &LayoutInput) -> (Layout, Net) {
        let l = layout::layout(input, &LayoutOptions::default());
        let net = Net::new(&l, &input.sizes);
        (l, net)
    }

    fn edge(child: u32, parent: u32) -> LayoutEdge {
        LayoutEdge {
            child,
            parent,
            first_parent: true,
        }
    }

    /// A chain of five nodes: 0 -> 1 -> 2 -> 3 -> 4.
    fn chain_net() -> (Layout, Net) {
        net_for(&LayoutInput {
            sizes: vec![Point::new(60.0, 20.0); 5],
            times: vec![5, 4, 3, 2, 1],
            edges: (0..4).map(|i| edge(i, i + 1)).collect(),
            priority: Vec::new(),
        })
    }

    fn settle(net: &mut Net, params: &NetParams) {
        for _ in 0..3000 {
            if !net.step(1.0 / 60.0, params) {
                return;
            }
        }
        panic!("net did not settle");
    }

    /// Drags `nodes` by `by` (grabbing `nodes[0]`) over half a second, then drops them.
    fn drag(net: &mut Net, nodes: &[usize], by: Point, params: &NetParams) {
        let start = net.node_pos(nodes[0]);
        net.grab(nodes[0], nodes, &[], params.model.adapts());
        for f in 1..=30 {
            let t = f as f32 / 30.0;
            net.drag_to(add(start, scale(by, t)));
            net.step(1.0 / 60.0, params);
        }
        net.release(params);
    }

    fn free() -> NetParams {
        NetParams {
            model: DragModel::Free,
            ..NetParams::default()
        }
    }

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 0.5 && (a.y - b.y).abs() < 0.5
    }

    #[test]
    fn neighbours_give_way_and_the_drop_stays() {
        let (l, mut net) = chain_net();
        let params = NetParams::default();
        let target = Point::new(l.nodes[2].x + 200.0, l.nodes[2].y);
        drag(&mut net, &[2], Point::new(200.0, 0.0), &params);
        settle(&mut net, &params);
        assert!(close(net.node_pos(2), target), "dropped node stays put");
        assert!(net.is_moved(2) && !net.is_moved(1));
        let moved = |net: &Net, i: usize| net.node_pos(i).x - l.nodes[i].x;
        assert!(
            moved(&net, 1) > 20.0 && moved(&net, 3) > 20.0,
            "neighbours follow"
        );
        assert!(
            moved(&net, 1) < 200.0 && moved(&net, 0) < moved(&net, 1),
            "pull decays: {:?}",
            (0..5).map(|i| moved(&net, i)).collect::<Vec<_>>()
        );
        // Nothing drifts afterwards: the new shape is the resting shape.
        let before: Vec<Point> = (0..5).map(|i| net.node_pos(i)).collect();
        for _ in 0..120 {
            net.step(1.0 / 60.0, &params);
        }
        assert!(!net.is_awake());
        for (i, &p) in before.iter().enumerate() {
            assert_eq!(net.node_pos(i), p);
        }
    }

    #[test]
    fn moved_nodes_keep_giving_way() {
        let (_, mut net) = chain_net();
        let params = NetParams::default();
        drag(&mut net, &[2], Point::new(200.0, 0.0), &params);
        settle(&mut net, &params);
        let dropped = net.node_pos(2);
        // Dragging a neighbour moves the node that was dropped before.
        drag(&mut net, &[3], Point::new(150.0, 0.0), &params);
        settle(&mut net, &params);
        assert!(
            net.node_pos(2).x > dropped.x + 20.0,
            "{:?} -> {:?}",
            dropped,
            net.node_pos(2)
        );
    }

    #[test]
    fn free_moves_only_the_dragged_nodes_and_stretches_their_edges() {
        // 0 -> 1 -> 2 -> 3, and a long edge 0 -> 3 with two bend points.
        let input = LayoutInput {
            sizes: vec![Point::new(60.0, 20.0); 4],
            times: vec![4, 3, 2, 1],
            edges: vec![edge(0, 1), edge(1, 2), edge(2, 3), edge(0, 3)],
            priority: Vec::new(),
        };
        let (l, mut net) = net_for(&input);
        let long = 3;
        assert_eq!(l.edges[long].len(), 4, "the long edge has two bend points");
        let by = Point::new(300.0, 0.0);
        drag(&mut net, &[0], by, &free());
        settle(&mut net, &free());
        assert!(close(net.node_pos(0), add(l.nodes[0], by)));
        for i in 1..4 {
            assert_eq!(net.node_pos(i), l.nodes[i], "node {i} stays");
        }
        // The bend points share the move, less the nearer they are to the parent.
        let pts: Vec<Point> = net.edge_points(long).collect();
        let shift = |k: usize| pts[k].x - l.edges[long][k].x;
        assert!(shift(1) > shift(2) && shift(2) > 0.0 && shift(1) < 300.0);
        assert!(net.is_moved(0));
    }

    #[test]
    fn free_moves_keep_their_offsets_when_adapting_again() {
        let (l, mut net) = chain_net();
        drag(&mut net, &[0], Point::new(300.0, 0.0), &free());
        settle(&mut net, &free());
        // Switching back to Adapt changes nothing by itself.
        let params = NetParams::default();
        for _ in 0..60 {
            net.step(1.0 / 60.0, &params);
        }
        assert!(close(
            net.node_pos(0),
            Point::new(l.nodes[0].x + 300.0, l.nodes[0].y)
        ));
        // Dragging its neighbour down pulls it along, but keeps it 300 to the right.
        drag(&mut net, &[1], Point::new(0.0, 60.0), &params);
        settle(&mut net, &params);
        let p = net.node_pos(0);
        assert!(p.y > l.nodes[0].y + 10.0, "follows: {p:?}");
        assert!(
            (p.x - l.nodes[0].x - 300.0).abs() < 5.0,
            "keeps its offset: {p:?}"
        );
    }

    #[test]
    fn carried_nodes_move_along_unmarked() {
        let (l, mut net) = chain_net();
        let by = Point::new(120.0, 0.0);
        net.grab(1, &[1], &[0, 1], false);
        net.drag_to(add(l.nodes[1], by));
        net.step(1.0 / 60.0, &free());
        net.release(&free());
        settle(&mut net, &free());
        for i in [0, 1] {
            assert!(
                close(net.node_pos(i), add(l.nodes[i], by)),
                "node {i} moves"
            );
        }
        assert!(
            net.is_moved(1) && !net.is_moved(0),
            "only the grabbed node is marked"
        );
        assert!(net.is_displaced(0));
    }

    #[test]
    fn selections_move_together() {
        let (l, mut net) = chain_net();
        let by = Point::new(-150.0, 30.0);
        drag(&mut net, &[1, 3], by, &free());
        settle(&mut net, &free());
        for i in [1, 3] {
            assert!(close(net.node_pos(i), add(l.nodes[i], by)));
        }
        for i in [0, 2, 4] {
            assert_eq!(net.node_pos(i), l.nodes[i]);
        }
    }

    #[test]
    fn nodes_push_each_other_away() {
        // Two separate chains side by side: 0 -> 1 and 2 -> 3.
        let input = LayoutInput {
            sizes: vec![Point::new(60.0, 20.0); 4],
            times: vec![4, 2, 3, 1],
            edges: vec![edge(0, 1), edge(2, 3)],
            priority: Vec::new(),
        };
        let (l, _) = net_for(&input);
        assert_eq!(l.nodes[0].y, l.nodes[2].y, "tips share a row");
        let (left, right) = if l.nodes[0].x < l.nodes[2].x {
            (0, 2)
        } else {
            (2, 0)
        };
        // Bring the left tip within 10 of the right one.
        let gap = l.nodes[right].x - l.nodes[left].x - 60.0;
        let by = Point::new(gap - 10.0, 0.0);
        let pushed = |push: f32| {
            let (_, mut net) = net_for(&input);
            let params = NetParams {
                push,
                ..NetParams::default()
            };
            drag(&mut net, &[left], by, &params);
            settle(&mut net, &params);
            net.node_pos(right).x - l.nodes[right].x
        };
        let (with, without) = (pushed(0.5), pushed(0.0));
        assert!(
            with > 5.0 && with > without + 3.0,
            "pushed aside: {with} vs {without}"
        );
    }

    #[test]
    fn undo_and_redo() {
        let (l, mut net) = chain_net();
        let params = NetParams::default();
        assert!(!net.can_undo());
        drag(&mut net, &[2], Point::new(200.0, 0.0), &params);
        settle(&mut net, &params);
        let shape: Vec<Point> = (0..5).map(|i| net.node_pos(i)).collect();
        assert!(net.undo());
        settle(&mut net, &params);
        for i in 0..5 {
            assert!(close(net.node_pos(i), l.nodes[i]), "node {i} back");
        }
        assert!(!net.is_moved(2) && !net.any_displaced());
        assert!(net.redo());
        settle(&mut net, &params);
        for (i, &p) in shape.iter().enumerate() {
            assert!(close(net.node_pos(i), p), "node {i} redone");
        }
        assert!(net.is_moved(2));
        // A reset is undoable too.
        net.reset();
        settle(&mut net, &params);
        assert!(close(net.node_pos(2), l.nodes[2]));
        assert!(net.undo());
        settle(&mut net, &params);
        assert!(close(net.node_pos(2), shape[2]));
    }

    #[test]
    fn undo_with_nothing_to_undo_keeps_the_drag() {
        let (l, mut net) = chain_net();
        net.grab(2, &[2], &[], true);
        assert!(!net.undo() && !net.redo());
        assert_eq!(net.grabbed(), Some(2));
        net.drag_to(add(l.nodes[2], Point::new(50.0, 0.0)));
        net.step(1.0 / 60.0, &NetParams::default());
        assert!(net.node_pos(2).x > l.nodes[2].x + 49.0);
    }

    #[test]
    fn grabbing_without_moving_changes_nothing() {
        let input = LayoutInput {
            sizes: vec![Point::new(60.0, 20.0); 6],
            times: vec![1, 6, 5, 4, 3, 2],
            edges: (1..6).map(|t| edge(t, 0)).collect(),
            priority: Vec::new(),
        };
        // Nodes closer together than the overlap margin, as the spacing settings allow.
        let options = LayoutOptions {
            node_gap: 3.0,
            ..LayoutOptions::default()
        };
        let l = layout::layout(&input, &options);
        let mut net = Net::new(&l, &input.sizes);
        let params = NetParams::default();
        let hold = |net: &mut Net, node: usize| {
            net.grab(node, &[node], &[], true);
            for _ in 0..20 {
                net.step(1.0 / 60.0, &params);
            }
            net.release(&params);
        };
        // While everything rests.
        hold(&mut net, 3);
        settle(&mut net, &params);
        assert_eq!(net.rest_offsets().count(), 0);
        assert!(!net.can_undo());
        // While a drop is still settling.
        drag(&mut net, &[3], Point::new(0.0, 40.0), &params);
        net.step(1.0 / 60.0, &params);
        let rest: Vec<(usize, Point, bool)> = net.rest_offsets().collect();
        for node in [3, 1, 4] {
            hold(&mut net, node);
        }
        settle(&mut net, &params);
        assert_eq!(net.rest_offsets().collect::<Vec<_>>(), rest);
        assert!(
            net.undo() && !net.can_undo(),
            "only the real drag was recorded"
        );
    }

    #[test]
    fn a_grab_wakes_only_what_it_presses_on() {
        // Many short chains side by side, rows closer than the magnet range.
        let chains = 3_000u32;
        let input = LayoutInput {
            sizes: vec![Point::new(40.0, 20.0); chains as usize * 3],
            times: (0..chains as i64 * 3).map(|i| 3 - i % 3).collect(),
            edges: (0..chains)
                .flat_map(|c| [edge(3 * c, 3 * c + 1), edge(3 * c + 1, 3 * c + 2)])
                .collect(),
            priority: Vec::new(),
        };
        let (_, mut net) = net_for(&input);
        net.grab(1, &[1], &[], true);
        for _ in 0..100 {
            net.step(1.0 / 60.0, &NetParams::default());
        }
        assert!(
            net.active_count() <= ACTIVE_BUDGET + 1,
            "{} particles awake",
            net.active_count()
        );
    }

    #[test]
    fn return_one_node_to_the_layout() {
        let (l, mut net) = chain_net();
        drag(&mut net, &[1, 2], Point::new(100.0, 0.0), &free());
        settle(&mut net, &free());
        net.return_to_layout(&[2]);
        settle(&mut net, &free());
        assert!(close(net.node_pos(2), l.nodes[2]) && !net.is_moved(2));
        assert!(close(
            net.node_pos(1),
            Point::new(l.nodes[1].x + 100.0, l.nodes[1].y)
        ));
        assert!(net.is_moved(1));
    }

    #[test]
    fn rest_offsets_round_trip() {
        let (l, mut net) = chain_net();
        drag(&mut net, &[3], Point::new(40.0, 0.0), &NetParams::default());
        settle(&mut net, &NetParams::default());
        let saved: Vec<(usize, Point, bool)> = net.rest_offsets().collect();
        assert!(saved.iter().any(|&(i, _, moved)| i == 3 && moved));
        let (_, mut fresh) = chain_net();
        fresh.restore(saved);
        assert!(!fresh.is_awake(), "restores at once");
        for i in 0..5 {
            assert!(close(fresh.node_pos(i), net.node_pos(i)), "node {i}");
        }
        assert!(fresh.is_moved(3) && !fresh.is_moved(2));
        assert!(
            fresh.node_pos(2).x > l.nodes[2].x,
            "neighbours as they were"
        );
    }

    #[test]
    fn only_the_neighbourhood_of_a_grab_is_simulated() {
        // A chain far longer than the active budget.
        let n = ACTIVE_BUDGET as u32 * 2;
        let input = LayoutInput {
            sizes: vec![Point::new(20.0, 10.0); n as usize],
            times: (0..n as i64).rev().collect(),
            edges: (0..n - 1).map(|i| edge(i, i + 1)).collect(),
            priority: Vec::new(),
        };
        let (l, mut net) = net_for(&input);
        net.grab(0, &[0], &[], true);
        assert!(net.active_count() <= ACTIVE_BUDGET + 1);
        net.drag_to(Point::new(l.nodes[0].x + 100.0, l.nodes[0].y));
        net.step(1.0 / 60.0, &NetParams::default());
        assert_eq!(
            net.node_pos(n as usize - 1),
            l.nodes[n as usize - 1],
            "far end untouched"
        );
        net.release(&NetParams::default());
        settle(&mut net, &NetParams::default());
        assert_eq!(net.active_count(), 0, "everything goes back to sleep");
    }

    #[test]
    fn pushing_wakes_resting_nodes_in_big_graphs() {
        // A row of more tips than are simulated at once, under one root.
        let tips = ACTIVE_BUDGET as u32 + 2_000;
        let input = LayoutInput {
            sizes: vec![Point::new(40.0, 20.0); tips as usize + 1],
            times: (0..=tips as i64).rev().collect(),
            edges: (0..tips).map(|t| edge(t, tips)).collect(),
            priority: Vec::new(),
        };
        let (l, mut net) = net_for(&input);
        let row = |i: usize| l.nodes[i].x;
        let first = (0..tips as usize)
            .min_by(|&a, &b| row(a).total_cmp(&row(b)))
            .unwrap();
        let params = NetParams::default();
        net.grab(first, &[first], &[], true);
        let asleep = (0..tips as usize).filter(|&i| !net.is_active[i]).count();
        assert!(asleep > 0, "part of the row is not simulated");
        // Sweep through the whole row in big jumps.
        let span = (0..tips as usize).map(row).fold(0.0f32, f32::max) - row(first);
        for f in 1..=40 {
            let x = row(first) + span * f as f32 / 40.0;
            net.drag_to(Point::new(x, l.nodes[first].y + 3.0));
            net.step(1.0 / 60.0, &params);
        }
        net.release(&params);
        settle(&mut net, &params);
        for i in 0..=tips as usize {
            let p = net.node_pos(i);
            assert!(p.x.is_finite() && p.y.is_finite());
        }
    }

    #[test]
    fn neighbours_in_a_layer_keep_their_order() {
        // Root 0 with three tips side by side in one layer.
        let input = LayoutInput {
            sizes: vec![Point::new(60.0, 20.0); 4],
            times: vec![1, 4, 3, 2],
            edges: (1..4).map(|t| edge(t, 0)).collect(),
            priority: Vec::new(),
        };
        let (l, _) = net_for(&input);
        let order = |net: &Net| {
            let mut tips = vec![1, 2, 3];
            tips.sort_by(|&a, &b| net.node_pos(a).x.total_cmp(&net.node_pos(b).x));
            tips
        };
        for params in [NetParams::default(), free()] {
            let (_, mut net) = net_for(&input);
            let before = order(&net);
            // Drag the leftmost tip far past the others and drop it there, then reset.
            let left = before[0];
            let by = Point::new(l.nodes[before[2]].x - l.nodes[left].x + 200.0, 0.0);
            drag(&mut net, &[left], by, &params);
            settle(&mut net, &params);
            if params.model.adapts() {
                assert_eq!(order(&net), before, "pushed ahead like beads on a string");
            }
            net.reset();
            settle(&mut net, &params);
            assert_eq!(order(&net), before);
            for i in 0..4 {
                assert!(close(net.node_pos(i), l.nodes[i]), "node {i} back home");
            }
        }
    }

    #[test]
    fn old_settings_still_load() {
        assert_eq!(
            serde_json_like("Net"),
            DragModel::Adapt,
            "the spider web became Adapt"
        );
        assert_eq!(serde_json_like("Rigid"), DragModel::Free);
    }

    /// Deserialises a unit variant by name without pulling in a format crate.
    fn serde_json_like(name: &str) -> DragModel {
        use serde::de::IntoDeserializer;
        use serde::de::value::{Error, StrDeserializer};
        let d: StrDeserializer<'_, Error> = name.into_deserializer();
        DragModel::deserialize(d).expect("known variant")
    }
}
