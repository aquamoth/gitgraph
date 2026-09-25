//! A proper layered graph: every edge spans exactly one layer, long edges being split into
//! chains of dummy items.

use super::LayoutInput;

/// Upper bound on dummy items; see [`LayeredGraph::build`].
const MAX_DUMMIES: u64 = 3_000_000;

/// A node or an edge dummy placed in one layer.
#[derive(Clone, Debug)]
pub struct Item {
    pub layer: u32,
    /// Extent along the layer.
    pub breadth: f32,
    /// True for a bend point of a long edge rather than a real node.
    pub dummy: bool,
    /// Neighbours in the layer above (newer) and below (older), with edge weights.
    pub up: Vec<(u32, f32)>,
    pub down: Vec<(u32, f32)>,
}

#[derive(Clone, Debug)]
pub struct LayeredGraph {
    /// Items `0..node_count` are the real nodes, in input order; dummies follow.
    pub items: Vec<Item>,
    pub node_count: usize,
    /// Items of each layer in left-to-right order.
    pub layers: Vec<Vec<u32>>,
    /// Position of every item within its layer (inverse of `layers`).
    pub pos: Vec<u32>,
    /// For every input edge, the dummy items it passes through, from child to parent.
    pub chains: Vec<Vec<u32>>,
}

/// Weight of an edge segment in coordinate assignment: long edges (dummy-to-dummy) are kept
/// straight most eagerly, then first-parent lines, then everything else.
fn segment_weight(first_parent: bool, from_dummy: bool, to_dummy: bool) -> f32 {
    let base = if first_parent { 2.0 } else { 1.0 };
    match (from_dummy, to_dummy) {
        (true, true) => base * 4.0,
        (true, false) | (false, true) => base * 2.0,
        (false, false) => base,
    }
}

impl LayeredGraph {
    /// Builds the layered graph. With `concentrate`, edges into the same parent share their
    /// dummy items wherever they pass through the same layer, so parallel edges merge into one
    /// trunk (graphviz's "edge concentration").
    pub fn build(
        input: &LayoutInput,
        layers: &[u32],
        breadth: &[f32],
        edge_gap: f32,
        concentrate: bool,
    ) -> Self {
        let n = input.sizes.len();
        let mut items: Vec<Item> = (0..n)
            .map(|i| Item {
                layer: layers[i],
                breadth: breadth[i],
                dummy: false,
                up: Vec::new(),
                down: Vec::new(),
            })
            .collect();
        let mut chains = Vec::with_capacity(input.edges.len());
        // Safety valve for pathological inputs: if routing every edge through every layer would
        // need more than MAX_DUMMIES bend points, the longest edges get none and are drawn as
        // direct lines.
        let spans: Vec<u32> = input
            .edges
            .iter()
            .map(|e| layers[e.parent as usize].saturating_sub(layers[e.child as usize]))
            .collect();
        let max_span = {
            let total: u64 = spans.iter().map(|&s| s.saturating_sub(1) as u64).sum();
            if total <= MAX_DUMMIES {
                u32::MAX
            } else {
                let mut sorted = spans.clone();
                sorted.sort_unstable();
                let mut budget = MAX_DUMMIES;
                let mut limit = 1;
                for &s in &sorted {
                    let cost = s.saturating_sub(1) as u64;
                    if cost > budget {
                        break;
                    }
                    budget -= cost;
                    limit = s;
                }
                limit
            }
        };
        // (parent, layer) -> shared dummy, when concentrating.
        let mut shared: std::collections::HashMap<(u32, u32), u32> =
            std::collections::HashMap::new();

        for e in &input.edges {
            let (c, p) = (e.child as usize, e.parent as usize);
            let mut chain = Vec::new();
            let mut prev = c as u32;
            if layers[p].saturating_sub(layers[c]) > max_span {
                // Too long to route: no bend points and no pull on the coordinates.
                chains.push(chain);
                continue;
            }
            for layer in layers[c] + 1..layers[p] {
                let existing = if concentrate {
                    shared.get(&(p as u32, layer)).copied()
                } else {
                    None
                };
                let d = match existing {
                    Some(d) => d,
                    None => {
                        let d = items.len() as u32;
                        items.push(Item {
                            layer,
                            breadth: edge_gap,
                            dummy: true,
                            up: Vec::new(),
                            down: Vec::new(),
                        });
                        if concentrate {
                            shared.insert((p as u32, layer), d);
                        }
                        d
                    }
                };
                link(&mut items, prev, d, e.first_parent);
                chain.push(d);
                prev = d;
            }
            if layers[p] > layers[c] {
                link(&mut items, prev, p as u32, e.first_parent);
            }
            chains.push(chain);
        }

        let layer_count = layers.iter().map(|&l| l as usize + 1).max().unwrap_or(0);
        let mut by_layer: Vec<Vec<u32>> = vec![Vec::new(); layer_count];
        for (i, item) in items.iter().enumerate() {
            by_layer[item.layer as usize].push(i as u32);
        }
        let mut g = LayeredGraph {
            pos: vec![0; items.len()],
            items,
            node_count: n,
            layers: by_layer,
            chains,
        };
        g.update_positions();
        g
    }

    /// Recomputes `pos` from `layers`.
    pub fn update_positions(&mut self) {
        for layer in &self.layers {
            for (i, &item) in layer.iter().enumerate() {
                self.pos[item as usize] = i as u32;
            }
        }
    }
}

fn link(items: &mut [Item], upper: u32, lower: u32, first_parent: bool) {
    // Concentrated edges share segments; link each pair only once (keeping the heavier weight).
    if let Some(existing) = items[upper as usize]
        .down
        .iter()
        .position(|&(d, _)| d == lower)
    {
        let w = segment_weight(
            first_parent,
            items[upper as usize].dummy,
            items[lower as usize].dummy,
        );
        let (_, old) = items[upper as usize].down[existing];
        if w > old {
            items[upper as usize].down[existing].1 = w;
            if let Some(u) = items[lower as usize]
                .up
                .iter_mut()
                .find(|(u, _)| *u == upper)
            {
                u.1 = w;
            }
        }
        return;
    }
    let w = segment_weight(
        first_parent,
        items[upper as usize].dummy,
        items[lower as usize].dummy,
    );
    items[upper as usize].down.push((lower, w));
    items[lower as usize].up.push((upper, w));
}
