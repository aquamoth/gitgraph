//! Crossing minimisation: ordering items within their layers.
//!
//! Starts from a depth-first order that keeps first-parent lines together, then applies
//! layer-by-layer median sweeps (Eades & Wormald; Gansner et al.) keeping the best ordering
//! seen, as measured by an exact crossing count (Barth, Jünger & Mutzel).

use super::{LayeredGraph, LayoutInput};

/// Maximum number of down+up sweep pairs.
const MAX_SWEEPS: usize = 24;
/// Stop after this many sweeps without improvement.
const PATIENCE: usize = 4;

pub fn minimize_crossings(g: &mut LayeredGraph, input: &LayoutInput) {
    initial_order(g, input);
    if g.layers.len() < 2 {
        return;
    }
    let mut best = g.layers.clone();
    let mut best_crossings = total_crossings(g);
    let mut stale = 0;
    for sweep in 0..MAX_SWEEPS {
        if best_crossings == 0 {
            break;
        }
        for l in 1..g.layers.len() {
            reorder_layer(g, l, true, sweep);
        }
        for l in (0..g.layers.len() - 1).rev() {
            reorder_layer(g, l, false, sweep);
        }
        let crossings = total_crossings(g);
        if crossings < best_crossings {
            best_crossings = crossings;
            best = g.layers.clone();
            stale = 0;
        } else {
            stale += 1;
            if stale >= PATIENCE {
                break;
            }
        }
    }
    g.layers = best;
    g.update_positions();
}

/// Depth-first from the tips (priority nodes first), descending into first parents before
/// other parents, so that each first-parent line forms a contiguous left-to-right band.
fn initial_order(g: &mut LayeredGraph, input: &LayoutInput) {
    let n_items = g.items.len();
    let mut seq = vec![u32::MAX; n_items];
    let mut next = 0u32;

    let mut starts: Vec<u32> = input.priority.clone();
    let mut by_age: Vec<u32> = (0..g.node_count as u32).collect();
    by_age.sort_by_key(|&v| {
        let time = input.times.get(v as usize).copied().unwrap_or(0);
        (g.items[v as usize].layer, std::cmp::Reverse(time))
    });
    starts.extend(by_age);

    let mut stack: Vec<(u32, usize)> = Vec::new();
    for start in starts {
        if seq[start as usize] != u32::MAX {
            continue;
        }
        seq[start as usize] = next;
        next += 1;
        stack.push((start, 0));
        while let Some(top) = stack.last_mut() {
            let (item, i) = (top.0 as usize, top.1);
            if let Some(&(child, _)) = g.items[item].down.get(i) {
                top.1 += 1;
                if seq[child as usize] == u32::MAX {
                    seq[child as usize] = next;
                    next += 1;
                    stack.push((child, 0));
                }
            } else {
                stack.pop();
            }
        }
    }
    for layer in &mut g.layers {
        layer.sort_by_key(|&i| seq[i as usize]);
    }
    g.update_positions();
}

/// Reorders layer `l` by the median position of each item's neighbours in the adjacent layer
/// (above when sweeping down). Items without such neighbours keep their slot.
fn reorder_layer(g: &mut LayeredGraph, l: usize, sweeping_down: bool, sweep: usize) {
    let mut scratch = Vec::new();
    let keys: Vec<Option<f32>> = g.layers[l]
        .iter()
        .map(|&i| {
            let item = &g.items[i as usize];
            let neighbours = if sweeping_down { &item.up } else { &item.down };
            scratch.clear();
            scratch.extend(neighbours.iter().map(|&(nb, _)| g.pos[nb as usize] as f32));
            median(&mut scratch)
        })
        .collect();

    let mut movable: Vec<(f32, usize, u32)> = g.layers[l]
        .iter()
        .zip(&keys)
        .enumerate()
        .filter_map(|(slot, (&item, key))| key.map(|k| (k, slot, item)))
        .collect();
    // Alternate the tie-breaking direction between sweeps so equal medians can swap.
    if sweep % 2 == 0 {
        movable.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    } else {
        movable.sort_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)));
    }
    let mut movable = movable.into_iter();
    let layer = &mut g.layers[l];
    for (slot, key) in keys.iter().enumerate() {
        if key.is_some() {
            layer[slot] = movable.next().expect("one movable item per keyed slot").2;
        }
    }
    for (i, &item) in layer.iter().enumerate() {
        g.pos[item as usize] = i as u32;
    }
}

/// Median with the Gansner et al. interpolation for even counts.
fn median(values: &mut [f32]) -> Option<f32> {
    let n = values.len();
    if n == 0 {
        return None;
    }
    values.sort_by(f32::total_cmp);
    let m = n / 2;
    Some(match n {
        _ if n % 2 == 1 => values[m],
        2 => (values[0] + values[1]) / 2.0,
        _ => {
            let left = values[m - 1] - values[0];
            let right = values[n - 1] - values[m];
            if left + right == 0.0 {
                (values[m - 1] + values[m]) / 2.0
            } else {
                (values[m - 1] * right + values[m] * left) / (left + right)
            }
        }
    })
}

/// Total number of edge crossings between all adjacent layer pairs.
pub fn total_crossings(g: &LayeredGraph) -> u64 {
    let mut edges: Vec<(u32, u32)> = Vec::new();
    let mut total = 0;
    for l in 0..g.layers.len().saturating_sub(1) {
        edges.clear();
        for &i in &g.layers[l] {
            for &(below, _) in &g.items[i as usize].down {
                edges.push((g.pos[i as usize], g.pos[below as usize]));
            }
        }
        total += bilayer_crossings(&mut edges, g.layers[l + 1].len());
    }
    total
}

/// Counts crossings among edges between two layers, given as (upper pos, lower pos), by
/// counting inversions of the lower positions with a Fenwick tree.
fn bilayer_crossings(edges: &mut [(u32, u32)], lower_len: usize) -> u64 {
    edges.sort_unstable();
    let mut tree = vec![0u64; lower_len + 1];
    let mut crossings = 0;
    for (seen, &(_, lower)) in edges.iter().enumerate() {
        // Edges seen so far with a lower endpoint strictly right of this one cross it.
        let mut not_greater = 0;
        let mut i = lower as usize + 1;
        while i > 0 {
            not_greater += tree[i];
            i &= i - 1;
        }
        crossings += seen as u64 - not_greater;
        let mut i = lower as usize + 1;
        while i <= lower_len {
            tree[i] += 1;
            i += i & i.wrapping_neg();
        }
    }
    crossings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_bilayer_crossings() {
        // Upper 0 -> lower 1, upper 1 -> lower 0: one crossing.
        assert_eq!(bilayer_crossings(&mut [(0, 1), (1, 0)], 2), 1);
        // Shared endpoints do not cross.
        assert_eq!(bilayer_crossings(&mut [(0, 0), (0, 1), (1, 1)], 2), 0);
        // Complete reversal of three edges: three crossings.
        assert_eq!(bilayer_crossings(&mut [(0, 2), (1, 1), (2, 0)], 3), 3);
    }

    #[test]
    fn median_interpolates() {
        assert_eq!(median(&mut []), None);
        assert_eq!(median(&mut [3.0]), Some(3.0));
        assert_eq!(median(&mut [4.0, 2.0]), Some(3.0));
        assert_eq!(median(&mut [1.0, 5.0, 3.0]), Some(3.0));
    }
}
