//! A proper layered graph: every edge spans exactly one layer, long edges being split into
//! chains of dummy items.

use super::LayoutInput;

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
    pub fn build(input: &LayoutInput, layers: &[u32], breadth: &[f32], edge_gap: f32) -> Self {
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

        for e in &input.edges {
            let (c, p) = (e.child as usize, e.parent as usize);
            let mut chain = Vec::new();
            let mut prev = c as u32;
            for layer in layers[c] + 1..layers[p] {
                let d = items.len() as u32;
                items.push(Item {
                    layer,
                    breadth: edge_gap,
                    dummy: true,
                    up: Vec::new(),
                    down: Vec::new(),
                });
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
    let w = segment_weight(
        first_parent,
        items[upper as usize].dummy,
        items[lower as usize].dummy,
    );
    items[upper as usize].down.push((lower, w));
    items[lower as usize].up.push((upper, w));
}
