//! A laid-out revision graph ready to draw: node contents and sizes, layout, and the physics
//! net that holds the current (possibly dragged) positions.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{Pos2, Rect, Vec2, pos2, vec2};
use gitgraph_core::layout::{self, Layout, LayoutEdge, LayoutInput, LayoutOptions, Point};
use gitgraph_core::physics::Net;
use gitgraph_core::revgraph::{self, RevGraph};
use gitgraph_core::{RefKind, Repo};

use crate::settings::Settings;

/// Node geometry at zoom 1, in logical pixels (TortoiseGit: Consolas 9 pt, margins 20 and 5).
pub const FONT_SIZE: f32 = 12.0;
pub const MARGIN_X: f32 = 20.0;
pub const MARGIN_Y: f32 = 5.0;
pub const CORNER_RADIUS: f32 = 6.0;
/// TortoiseGit shows 8 hex digits for commits without refs.
pub const HASH_DIGITS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowKind {
    Hash,
    Ref { kind: RefKind, head: bool },
}

#[derive(Clone, Debug)]
pub struct Row {
    pub label: String,
    pub kind: RowKind,
}

#[derive(Clone, Debug)]
pub struct NodeVisual {
    pub rows: Vec<Row>,
    pub size: Vec2,
}

#[derive(Debug)]
pub struct Scene {
    /// The repository snapshot this scene was built from. Everything that maps nodes back to
    /// commits and refs must use this one (not a newer reload).
    pub repo: Arc<Repo>,
    pub graph: RevGraph,
    pub layout: Layout,
    pub visuals: Vec<NodeVisual>,
    pub net: Net,
    pub row_height: f32,
    pub build_time: Duration,
    pub layout_time: Duration,
}

/// Everything needed to lay a scene out, prepared on the UI thread (which owns the fonts);
/// [`SceneInput::lay_out`] can then run on any thread.
#[derive(Debug)]
pub struct SceneInput {
    repo: Arc<Repo>,
    graph: RevGraph,
    visuals: Vec<NodeVisual>,
    input: LayoutInput,
    options: LayoutOptions,
    row_height: f32,
    build_time: Duration,
}

impl SceneInput {
    pub fn lay_out(self) -> Scene {
        let t = Instant::now();
        let layout = layout::layout(&self.input, &self.options);
        let net = Net::new(&layout, &self.input.sizes);
        Scene {
            repo: self.repo,
            graph: self.graph,
            layout,
            visuals: self.visuals,
            net,
            row_height: self.row_height,
            build_time: self.build_time,
            layout_time: t.elapsed(),
        }
    }
}

impl Scene {
    /// Builds the graph for the current settings and measures its nodes. `text_width`
    /// measures a string at [`FONT_SIZE`]; `text_height` is the height of one line of text.
    pub fn prepare(
        repo: &Arc<Repo>,
        settings: &Settings,
        text_width: &mut dyn FnMut(&str) -> f32,
        text_height: f32,
    ) -> SceneInput {
        let t = Instant::now();
        let graph = revgraph::build(repo, &settings.graph);
        let build_time = t.elapsed();

        let row_height = text_height + 2.0 * MARGIN_Y;
        let hash_width = text_width(&"8".repeat(HASH_DIGITS));
        let visuals: Vec<NodeVisual> = graph
            .nodes
            .iter()
            .map(|node| {
                let rows: Vec<Row> = if node.refs.is_empty() {
                    vec![Row {
                        label: repo.commit(node.commit).oid.short(HASH_DIGITS),
                        kind: RowKind::Hash,
                    }]
                } else {
                    node.refs
                        .iter()
                        .map(|&i| {
                            let r = &repo.refs[i];
                            Row {
                                label: r.name.clone(),
                                kind: RowKind::Ref {
                                    kind: r.kind,
                                    head: r.is_head,
                                },
                            }
                        })
                        .collect()
                };
                let widest = rows
                    .iter()
                    .map(|r| text_width(&r.label))
                    .fold(hash_width, f32::max);
                let size = vec2(widest + 2.0 * MARGIN_X, row_height * rows.len() as f32);
                NodeVisual { rows, size }
            })
            .collect();

        let sizes: Vec<Point> = visuals
            .iter()
            .map(|v| Point::new(v.size.x, v.size.y))
            .collect();
        let head = graph.nodes.iter().position(|n| n.is_head);
        let input = LayoutInput {
            sizes: sizes.clone(),
            times: graph
                .nodes
                .iter()
                .map(|n| repo.commit(n.commit).commit_time)
                .collect(),
            edges: graph
                .edges
                .iter()
                .map(|e| LayoutEdge {
                    child: e.child,
                    parent: e.parent,
                    first_parent: e.first_parent,
                })
                .collect(),
            priority: head.map(|h| vec![h as u32]).unwrap_or_default(),
        };
        SceneInput {
            repo: Arc::clone(repo),
            graph,
            visuals,
            input,
            options: settings.layout.clone(),
            row_height,
            build_time,
        }
    }

    pub fn node_count(&self) -> usize {
        self.visuals.len()
    }

    /// Current centre of a node in world coordinates.
    pub fn node_center(&self, node: usize) -> Pos2 {
        to_pos(self.net.node_pos(node))
    }

    /// Current box of a node in world coordinates.
    pub fn node_rect(&self, node: usize) -> Rect {
        Rect::from_center_size(self.node_center(node), self.visuals[node].size)
    }

    /// Topmost node under a world position.
    pub fn node_at(&self, world: Pos2) -> Option<usize> {
        (0..self.node_count())
            .rev()
            .find(|&i| self.node_rect(i).contains(world))
    }

    /// Bounding box of the drawing in world coordinates (including dragged nodes).
    pub fn bounds(&self) -> Rect {
        let mut r = Rect::from_min_max(to_pos(self.layout.min), to_pos(self.layout.max));
        for i in 0..self.node_count() {
            r = r.union(self.node_rect(i));
        }
        r
    }

    pub fn head_node(&self) -> Option<usize> {
        self.graph.nodes.iter().position(|n| n.is_head)
    }
}

pub fn to_pos(p: Point) -> Pos2 {
    pos2(p.x, p.y)
}

pub fn to_point(p: Pos2) -> Point {
    Point::new(p.x, p.y)
}
