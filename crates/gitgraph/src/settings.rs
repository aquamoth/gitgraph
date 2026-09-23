//! User-adjustable settings, persisted between runs by eframe.

use gitgraph_core::layout::LayoutOptions;
use gitgraph_core::physics::NetParams;
use gitgraph_core::revgraph::GraphOptions;
use serde::{Deserialize, Serialize};

use crate::theme::ThemeChoice;

pub const STORAGE_KEY: &str = "gitgraph-settings";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeStyle {
    /// Straight segments through the bend points, as TortoiseGit draws them.
    #[default]
    Straight,
    /// Smooth curves that leave and enter nodes along the direction of history.
    Curved,
}

impl EdgeStyle {
    pub const ALL: [EdgeStyle; 2] = [EdgeStyle::Straight, EdgeStyle::Curved];

    pub fn label(self) -> &'static str {
        match self {
            EdgeStyle::Straight => "Straight (TortoiseGit)",
            EdgeStyle::Curved => "Curved",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Arrows {
    /// Arrowhead at the parent (older) end, TortoiseGit's default.
    #[default]
    ToParent,
    /// Arrowhead at the child end: "Arrows point towards merges".
    ToChild,
    None,
}

impl Arrows {
    pub const ALL: [Arrows; 3] = [Arrows::ToParent, Arrows::ToChild, Arrows::None];

    pub fn label(self) -> &'static str {
        match self {
            Arrows::ToParent => "Point to parents",
            Arrows::ToChild => "Point towards merges",
            Arrows::None => "No arrows",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub graph: GraphOptions,
    pub layout: LayoutOptions,
    pub net: NetParams,
    pub theme: ThemeChoice,
    pub edge_style: EdgeStyle,
    pub arrows: Arrows,
    /// Overview map of the whole graph in the bottom-right corner.
    pub show_overview: bool,
    /// Label edges with the number of commits collapsed into them.
    pub show_hidden_counts: bool,
    /// Highlight the edges of the hovered and selected nodes.
    pub highlight_edges: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            graph: GraphOptions::default(),
            layout: LayoutOptions::default(),
            net: NetParams::default(),
            theme: ThemeChoice::default(),
            edge_style: EdgeStyle::default(),
            arrows: Arrows::default(),
            show_overview: false,
            show_hidden_counts: false,
            highlight_edges: true,
        }
    }
}
