//! User-adjustable settings, persisted between runs by eframe.

use parterre_core::layout::LayoutOptions;
use parterre_core::physics::NetParams;
use parterre_core::revgraph::GraphOptions;
use serde::{Deserialize, Serialize};

use crate::theme::{BranchColor, ThemeChoice};

/// The app id: names eframe's storage directory and, on Wayland, the window (matching
/// `packaging/linux/parterre.desktop`).
pub const APP_ID: &str = "parterre";
/// What the app was called up to 0.2, and its app id then.
const OLD_APP_ID: &str = "gitgraph";

// The storage keys still carry the old name, so settings saved before the rename keep loading.
pub const STORAGE_KEY: &str = "gitgraph-settings";
/// Storage key for remembered node positions: repository path -> commit hash -> rest offset
/// from the layout, and whether the node was moved by hand.
pub const MOVES_KEY: &str = "gitgraph-rest-offsets";
/// Storage key for the recently opened repositories, newest first.
pub const RECENT_KEY: &str = "parterre-recent-repositories";
/// The format before nodes gave way to each other: only dropped (pinned) nodes and offsets.
const OLD_MOVES_KEY: &str = "gitgraph-moved-nodes";

pub type RememberedMoves =
    std::collections::HashMap<String, std::collections::HashMap<String, (f32, f32, bool)>>;

/// Carries over what the app saved while it was called gitgraph: the first time it runs as
/// parterre, it copies the old storage directory's `app.ron`, the one file eframe keeps there.
pub fn adopt_old_storage() {
    if let (Some(from), Some(to)) = (eframe::storage_dir(OLD_APP_ID), eframe::storage_dir(APP_ID)) {
        copy_storage(&from, &to);
    }
}

/// Copies `app.ron` from one storage directory into another that has none yet. Best effort:
/// when it fails, the app starts with default settings.
fn copy_storage(from: &std::path::Path, to: &std::path::Path) {
    let (old, new) = (from.join("app.ron"), to.join("app.ron"));
    if old.exists() && !new.exists() {
        let _ = std::fs::create_dir_all(to).and_then(|()| std::fs::copy(&old, &new));
    }
}

/// Loads remembered node positions, converting the older format.
pub fn load_moves(storage: &dyn eframe::Storage) -> RememberedMoves {
    if let Some(moves) = eframe::get_value(storage, MOVES_KEY) {
        return moves;
    }
    let old: std::collections::HashMap<String, std::collections::HashMap<String, (f32, f32)>> =
        eframe::get_value(storage, OLD_MOVES_KEY).unwrap_or_default();
    old.into_iter()
        .map(|(repo, nodes)| {
            let nodes = nodes
                .into_iter()
                .map(|(hex, (dx, dy))| (hex, (dx, dy, true)))
                .collect();
            (repo, nodes)
        })
        .collect()
}

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
            EdgeStyle::Straight => "Straight",
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

/// Bundles of drawing choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// As close to TortoiseGit as possible: straight edges, every edge separate, rows as wide
    /// as they need to be.
    Classic,
    /// parterre's default: curved edges bundled into trunks, and very wide rows split so that
    /// sibling branches stack up.
    Modern,
}

impl Look {
    pub const ALL: [Look; 2] = [Look::Modern, Look::Classic];

    pub fn label(self) -> &'static str {
        match self {
            Look::Classic => "Classic (TortoiseGit)",
            Look::Modern => "Modern",
        }
    }

    pub fn apply(self, s: &mut Settings) {
        let defaults = LayoutOptions::default();
        match self {
            Look::Classic => {
                s.edge_style = EdgeStyle::Straight;
                s.layout.concentrate_edges = false;
                s.layout.max_layer_width = 0.0;
            }
            Look::Modern => {
                s.edge_style = EdgeStyle::Curved;
                s.layout.concentrate_edges = true;
                s.layout.max_layer_width = defaults.max_layer_width;
            }
        }
    }

    /// The look `s` currently matches, if any.
    pub fn of(s: &Settings) -> Option<Look> {
        Look::ALL.into_iter().find(|look| {
            let mut probe = s.clone();
            look.apply(&mut probe);
            probe == *s
        })
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
    pub show_status_bar: bool,
    /// Label edges with the number of commits collapsed into them.
    pub show_hidden_counts: bool,
    /// Highlight the edges of the hovered and selected nodes.
    pub highlight_edges: bool,
    /// Keep moved nodes where they are, per repository, across runs and relayouts.
    pub remember_moves: bool,
    /// Colours for branches by name; the first matching rule wins.
    pub branch_colors: Vec<BranchColor>,
}

impl Default for Settings {
    fn default() -> Self {
        let mut s = Settings {
            graph: GraphOptions::default(),
            layout: LayoutOptions::default(),
            net: NetParams::default(),
            theme: ThemeChoice::default(),
            edge_style: EdgeStyle::default(),
            arrows: Arrows::default(),
            show_overview: false,
            show_status_bar: true,
            show_hidden_counts: false,
            highlight_edges: true,
            remember_moves: false,
            branch_colors: Vec::new(),
        };
        Look::Modern.apply(&mut s);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_old_storage_once() {
        let tmp = tempfile::tempdir().unwrap();
        let (from, to) = (tmp.path().join("old"), tmp.path().join("new"));
        std::fs::create_dir(&from).unwrap();
        std::fs::write(from.join("app.ron"), "saved").unwrap();
        copy_storage(&from, &to);
        assert_eq!(
            std::fs::read_to_string(to.join("app.ron")).unwrap(),
            "saved"
        );

        // Once the new directory has its own file, the old one is left alone.
        std::fs::write(from.join("app.ron"), "saved later").unwrap();
        copy_storage(&from, &to);
        assert_eq!(
            std::fs::read_to_string(to.join("app.ron")).unwrap(),
            "saved"
        );
    }
}
