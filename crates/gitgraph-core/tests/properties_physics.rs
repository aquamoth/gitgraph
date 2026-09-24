//! Property tests: random graphs, random drags. Positions stay finite, the net settles, a reset
//! returns every node to its layout position, and children stay above their parents. (Adapted
//! from a review's fuzzing.)

#![allow(clippy::needless_range_loop)] // index loops read better in these tests

use gitgraph_core::layout::{
    self, Direction, LayoutEdge, LayoutInput, LayoutOptions, Point, Ranking,
};
use gitgraph_core::physics::{DragModel, FLOW_GAP, Net, NetParams};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
    fn f(&mut self) -> f32 {
        (self.next() % 10_000) as f32 / 10_000.0
    }
}

fn input(rng: &mut Rng, n: usize) -> LayoutInput {
    let mut edges = Vec::new();
    for c in 0..n {
        for k in 0..rng.below(3) {
            if c + 1 < n {
                let p = (c + 1 + rng.below(4) as usize).min(n - 1);
                if !edges
                    .iter()
                    .any(|e: &LayoutEdge| e.child == c as u32 && e.parent == p as u32)
                {
                    edges.push(LayoutEdge {
                        child: c as u32,
                        parent: p as u32,
                        first_parent: k == 0,
                    });
                }
            }
        }
    }
    LayoutInput {
        sizes: (0..n)
            .map(|_| Point::new(40.0 + rng.below(100) as f32, 22.0))
            .collect(),
        times: vec![],
        edges,
        priority: vec![],
    }
}

#[test]
fn physics_random_drags() {
    let mut rng = Rng(4242);
    for iter in 0..120 {
        let n = 1 + rng.below(60) as usize;
        let inp = input(&mut rng, n);
        let opts = LayoutOptions {
            concentrate_edges: iter % 2 == 0,
            ranking: Ranking::ALL[iter % 3],
            direction: Direction::ALL[iter % 4],
            ..LayoutOptions::default()
        };
        let l = layout::layout(&inp, &opts);
        let mut net = Net::new(&l, &inp.sizes);
        let params = NetParams {
            model: DragModel::ALL[iter % 3],
            reach: rng.f(),
            wobble: rng.f(),
            avoid_overlap: iter % 5 != 0,
        };
        for _ in 0..5 {
            let node = rng.below(n as u64) as usize;
            net.grab(node);
            for s in 0..20 {
                let t = Point::new(
                    l.nodes[node].x + (s as f32) * 20.0 * (rng.f() - 0.5),
                    l.nodes[node].y + (s as f32) * 20.0 * (rng.f() - 0.5),
                );
                net.drag_to(t);
                let dt = [1.0 / 60.0, 0.0, 1.0, 1e-6, 1.0 / 240.0][rng.below(5) as usize];
                net.step(dt, &params);
            }
            net.release();
            if rng.below(3) == 0 {
                net.unpin(node);
            }
            for _ in 0..30 {
                net.step(1.0 / 60.0, &params);
            }
        }
        for i in 0..n {
            let p = net.node_pos(i);
            assert!(
                p.x.is_finite() && p.y.is_finite(),
                "iter {iter}: NaN node {i}"
            );
        }
        for e in 0..inp.edges.len() {
            let pts: Vec<Point> = net.edge_points(e).collect();
            assert!(pts.len() >= 2);
            assert!(
                pts.iter().all(|p| p.x.is_finite() && p.y.is_finite()),
                "iter {iter}: NaN edge"
            );
        }
        net.reset();
        let mut settled = false;
        for _ in 0..5000 {
            if !net.step(1.0 / 60.0, &params) {
                settled = true;
                break;
            }
        }
        assert!(settled, "iter {iter}: never settles after reset");
        // Everything returns to the layout after a reset.
        let residual = (0..n)
            .map(|i| {
                let p = net.node_pos(i);
                (p.x - l.nodes[i].x).abs().max((p.y - l.nodes[i].y).abs())
            })
            .fold(0.0f32, f32::max);
        assert!(
            residual < 1.0,
            "iter {iter} {params:?}: {residual} px off after reset"
        );
    }
}

#[test]
fn physics_keeps_children_above_parents() {
    let mut rng = Rng(777);
    for iter in 0..120 {
        let n = 2 + rng.below(50) as usize;
        let inp = input(&mut rng, n);
        let opts = LayoutOptions {
            concentrate_edges: iter % 2 == 0,
            ranking: Ranking::ALL[iter % 3],
            direction: Direction::ALL[iter % 4],
            ..LayoutOptions::default()
        };
        let l = layout::layout(&inp, &opts);
        let mut net = Net::new(&l, &inp.sizes);
        let params = NetParams {
            model: [DragModel::Net, DragModel::Strings][iter % 2],
            reach: rng.f(),
            wobble: rng.f(),
            avoid_overlap: iter % 3 != 0,
        };
        // Drag one node far along or against the flow (and a bit sideways), and drop it.
        let node = rng.below(n as u64) as usize;
        let f = opts.direction.flow();
        let (along, side) = (1200.0 * (rng.f() - 0.5), 200.0 * (rng.f() - 0.5));
        net.grab(node);
        for s in 1..=20 {
            let t = s as f32 / 20.0;
            net.drag_to(Point::new(
                l.nodes[node].x + t * (along * f.x + side * f.y),
                l.nodes[node].y + t * (along * f.y + side * f.x),
            ));
            net.step(1.0 / 60.0, &params);
        }
        net.release();
        // Not required to settle: overlap avoidance can keep a pinned net trembling by a
        // fraction of a pixel (see TODO.md).
        for _ in 0..5000 {
            if !net.step(1.0 / 60.0, &params) {
                break;
            }
        }
        let depth = |i: usize| {
            let s = inp.sizes[i];
            (f.x.abs() * s.x + f.y.abs() * s.y) / 2.0
        };
        let along_flow = |p: Point| p.x * f.x + p.y * f.y;
        for e in &inp.edges {
            let (c, p) = (e.child as usize, e.parent as usize);
            if c == p {
                continue;
            }
            let needed = (depth(c) + depth(p) + FLOW_GAP)
                .min(along_flow(l.nodes[p]) - along_flow(l.nodes[c]));
            let have = along_flow(net.node_pos(p)) - along_flow(net.node_pos(c));
            assert!(
                have >= needed - 1.0,
                "iter {iter} {params:?}, dragged {node} by {along}: edge {c} -> {p} \
                 has {have} along the flow, needs {needed}"
            );
        }
    }
}
