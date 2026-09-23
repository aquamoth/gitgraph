//! Coordinate assignment along layers.
//!
//! Minimises `Σ w·|u(a) − u(b)|` over all edge segments, subject to each layer keeping its
//! order with minimum separations, i.e. the objective of OGDF's `OptimalHierarchyLayout`. We
//! solve it by block coordinate descent: each layer in turn is placed optimally given its
//! neighbours, which is a weighted isotonic regression solved exactly by pool-adjacent-
//! violators. The L1 objective is approached by iteratively reweighted least squares, which
//! makes edges exactly straight wherever the constraints allow.

use super::LayeredGraph;

const SWEEPS: usize = 40;
/// Below this distance (layout units) an edge counts as straight for reweighting.
const STRAIGHT: f32 = 1.0;
/// Pull of an item towards its previous position, relative to its edge weights.
const INERTIA: f32 = 0.02;

/// Returns the along-layer centre coordinate of every item.
pub fn assign(g: &LayeredGraph, node_gap: f32) -> Vec<f32> {
    let mut u = vec![0.0f32; g.items.len()];
    // Start packed to the left.
    for layer in &g.layers {
        let mut x = 0.0;
        for (k, &i) in layer.iter().enumerate() {
            if k > 0 {
                x += separation(g, layer[k - 1], i, node_gap);
            }
            u[i as usize] = x;
        }
    }

    let mut targets = Vec::new();
    let mut weights = Vec::new();
    let mut seps = Vec::new();
    for sweep in 0..SWEEPS {
        // L2 for the first sweeps to settle, then reweight towards L1.
        let l1 = sweep >= SWEEPS / 4;
        let order: Box<dyn Iterator<Item = usize>> = if sweep % 2 == 0 {
            Box::new(0..g.layers.len())
        } else {
            Box::new((0..g.layers.len()).rev())
        };
        for l in order {
            let layer = &g.layers[l];
            targets.clear();
            weights.clear();
            seps.clear();
            for (k, &i) in layer.iter().enumerate() {
                let item = &g.items[i as usize];
                let x = u[i as usize];
                let mut sum_w = 0.0;
                let mut sum_wx = 0.0;
                for &(nb, w) in item.up.iter().chain(&item.down) {
                    let nx = u[nb as usize];
                    let w = if l1 {
                        w / (x - nx).abs().max(STRAIGHT)
                    } else {
                        w
                    };
                    sum_w += w;
                    sum_wx += w * nx;
                }
                let inertia = INERTIA * sum_w.max(1.0);
                targets.push((sum_wx + inertia * x) / (sum_w + inertia));
                weights.push(sum_w + inertia);
                if k > 0 {
                    seps.push(separation(g, layer[k - 1], i, node_gap));
                }
            }
            let placed = isotonic(&targets, &weights, &seps);
            for (k, &i) in layer.iter().enumerate() {
                u[i as usize] = placed[k];
            }
        }
    }
    u
}

/// Minimum centre-to-centre distance between adjacent items `a` (left) and `b`.
fn separation(g: &LayeredGraph, a: u32, b: u32, node_gap: f32) -> f32 {
    let (ia, ib) = (&g.items[a as usize], &g.items[b as usize]);
    let gap = match (ia.dummy, ib.dummy) {
        (true, true) => 0.0,
        (false, false) => node_gap,
        _ => node_gap * 0.5,
    };
    (ia.breadth + ib.breadth) / 2.0 + gap
}

/// Weighted least-squares placement `x` minimising `Σ w_i (x_i − t_i)²` subject to
/// `x_{i+1} − x_i ≥ sep_i`, by pool-adjacent-violators on `y_i = x_i − Σ_{j<i} sep_j`.
fn isotonic(targets: &[f32], weights: &[f32], seps: &[f32]) -> Vec<f32> {
    let n = targets.len();
    let mut offset = Vec::with_capacity(n);
    let mut acc = 0.0f64;
    for i in 0..n {
        if i > 0 {
            acc += seps[i - 1] as f64;
        }
        offset.push(acc);
    }
    // Blocks of (sum w·y, sum w, item count).
    let mut blocks: Vec<(f64, f64, usize)> = Vec::with_capacity(n);
    for i in 0..n {
        let w = weights[i].max(1e-6) as f64;
        let y = targets[i] as f64 - offset[i];
        blocks.push((w * y, w, 1));
        while blocks.len() >= 2 {
            let (b, a) = (blocks[blocks.len() - 1], blocks[blocks.len() - 2]);
            if a.0 / a.1 <= b.0 / b.1 {
                break;
            }
            blocks.pop();
            let last = blocks.last_mut().unwrap();
            *last = (a.0 + b.0, a.1 + b.1, a.2 + b.2);
        }
    }
    let mut x = Vec::with_capacity(n);
    for (swy, sw, count) in blocks {
        let y = swy / sw;
        for _ in 0..count {
            x.push((y + offset[x.len()]) as f32);
        }
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isotonic_keeps_feasible_targets() {
        let x = isotonic(&[0.0, 10.0, 30.0], &[1.0; 3], &[5.0, 5.0]);
        assert_eq!(x, vec![0.0, 10.0, 30.0]);
    }

    #[test]
    fn isotonic_pools_violators_around_weighted_mean() {
        // Both want 0 but must be 10 apart: centred on 0.
        let x = isotonic(&[0.0, 0.0], &[1.0, 1.0], &[10.0]);
        assert_eq!(x, vec![-5.0, 5.0]);
        // Heavier item stays closer to its target.
        let x = isotonic(&[0.0, 0.0], &[3.0, 1.0], &[8.0]);
        assert_eq!(x, vec![-2.0, 6.0]);
    }
}
