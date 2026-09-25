//! The eframe application: menus, toolbar, canvas interaction and status bar.

use std::path::PathBuf;

use eframe::egui::{
    self, Color32, FontId, Key, Modifiers, PointerButton, Pos2, Rect, RichText, Sense, Ui, Vec2,
    vec2,
};
use gitgraph_core::layout::{Direction, LayoutOptions, Ranking};
use gitgraph_core::physics::DragModel;
use gitgraph_core::revgraph::{GraphOptions, Simplification};
use std::sync::Arc;

use gitgraph_core::{CommitIx, Oid, Repo};

use crate::automation::Automation;
use crate::render::{self, Marks};
use crate::scene::{FONT_SIZE, Scene, to_point};
use crate::settings::{
    Arrows, EdgeStyle, Look, MOVES_KEY, RememberedMoves, STORAGE_KEY, Settings, load_moves,
};
use crate::theme::{BranchColor, Palette, ThemeChoice};
use crate::view::View;

#[derive(Clone, Copy, Debug)]
enum Drag {
    /// Dragging nodes; `grab` is the pointer's offset from the grabbed node's centre (world
    /// units).
    Node {
        grab: Vec2,
    },
    Pan,
    /// Selecting the nodes in the rectangle between `start` and `end` (world coordinates).
    Select {
        start: Pos2,
        end: Pos2,
    },
}

/// Selected nodes, in the order they were selected. The last one is the current node, whose
/// details the status bar shows.
#[derive(Clone, Debug, Default)]
struct Selection {
    nodes: Vec<usize>,
}

impl Selection {
    fn current(&self) -> Option<usize> {
        self.nodes.last().copied()
    }

    fn contains(&self, node: usize) -> bool {
        self.nodes.contains(&node)
    }

    fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Selects `node` alone, or nothing.
    fn set(&mut self, node: Option<usize>) {
        self.nodes.clear();
        self.nodes.extend(node);
    }

    /// Adds `node`, making it the current node.
    fn add(&mut self, node: usize) {
        self.nodes.retain(|&n| n != node);
        self.nodes.push(node);
    }

    /// Adds `nodes` that are not selected yet, keeping the current node current.
    fn extend(&mut self, nodes: impl IntoIterator<Item = usize>) {
        let mut seen: std::collections::HashSet<usize> = self.nodes.iter().copied().collect();
        let current = self.nodes.pop();
        self.nodes.extend(
            nodes
                .into_iter()
                .filter(|&n| Some(n) != current && seen.insert(n)),
        );
        self.nodes.extend(current);
    }

    fn toggle(&mut self, node: usize) {
        if self.contains(node) {
            self.nodes.retain(|&n| n != node);
        } else {
            self.nodes.push(node);
        }
    }

    /// Per node of a scene with `count` nodes: selected.
    fn mask(&self, count: usize) -> Vec<bool> {
        let mut mask = vec![false; count];
        for &n in &self.nodes {
            if n < count {
                mask[n] = true;
            }
        }
        mask
    }
}

/// The nodes dragged along with `anchor`: the selection it belongs to, or `anchor` alone.
fn dragged_with(selection: &Selection, anchor: usize) -> Vec<usize> {
    if selection.contains(anchor) {
        selection.nodes.clone()
    } else {
        vec![anchor]
    }
}

/// Full commit messages for tooltips, fetched from git on a worker thread when first needed.
#[derive(Debug, Default)]
struct Messages {
    /// `None` while loading.
    cache: std::collections::HashMap<gitgraph_core::Oid, Option<String>>,
    rx: Option<std::sync::mpsc::Receiver<(gitgraph_core::Oid, String)>>,
    tx: Option<std::sync::mpsc::Sender<gitgraph_core::Oid>>,
}

impl Messages {
    /// The message of `oid` if already loaded; otherwise requests it.
    fn get(
        &mut self,
        repo_path: &std::path::Path,
        oid: gitgraph_core::Oid,
        ctx: &egui::Context,
    ) -> Option<&str> {
        while let Some(Ok((oid, msg))) = self.rx.as_ref().map(|rx| rx.try_recv()) {
            self.cache.insert(oid, Some(msg));
        }
        if let std::collections::hash_map::Entry::Vacant(slot) = self.cache.entry(oid) {
            slot.insert(None);
            let tx = self.tx.get_or_insert_with(|| {
                let (req_tx, req_rx) = std::sync::mpsc::channel::<gitgraph_core::Oid>();
                let (res_tx, res_rx) = std::sync::mpsc::channel();
                let git = gitgraph_core::git::Git::new(repo_path);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    for oid in req_rx {
                        let msg = git.message(&oid).unwrap_or_else(|e| format!("({e})"));
                        if res_tx.send((oid, msg)).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                });
                self.rx = Some(res_rx);
                req_tx
            });
            let _ = tx.send(oid);
        }
        self.cache.get(&oid).and_then(|m| m.as_deref())
    }
}

/// A layout running on a worker thread.
#[derive(Debug)]
struct LayoutJob {
    rx: std::sync::mpsc::Receiver<Scene>,
    /// Commit near the view centre and its screen position, to keep the view steady.
    anchor: Option<(Oid, Pos2)>,
    selected_commits: Vec<Oid>,
}

#[derive(Debug, Default)]
struct Search {
    query: String,
    hits: Vec<usize>,
    current: Option<usize>,
    request_focus: bool,
}

pub struct GitGraphApp {
    repo_path: PathBuf,
    /// The most recently loaded snapshot, used for new layouts. The scene on screen keeps its
    /// own snapshot until a new layout replaces it.
    repo: Arc<Repo>,
    settings: Settings,
    /// False for automated runs, so they don't overwrite the user's settings.
    persist: bool,
    scene: Option<Scene>,
    /// Options of the most recently requested layout.
    requested: Option<(GraphOptions, LayoutOptions)>,
    job: Option<LayoutJob>,
    view: View,
    needs_initial_view: bool,
    canvas: Rect,
    hovered: Option<usize>,
    hovered_edge: Option<usize>,
    selection: Selection,
    /// Nodes that would move if the hovered node were dragged in Subtree mode, cached for
    /// the roots they were computed from.
    preview: Option<(Vec<usize>, Vec<usize>)>,
    context_node: Option<usize>,
    /// Commits to select once the scene has been rebuilt (after a reload).
    pending_select: Vec<Oid>,
    drag: Option<Drag>,
    search: Search,
    status: Option<(String, bool)>,
    show_shortcuts: bool,
    show_legend: bool,
    show_branch_colors: bool,
    show_about: bool,
    /// Path being edited in the "Export as SVG" dialog, when open.
    export_path: Option<String>,
    messages: Messages,
    /// Dragged nodes of every repository, kept when `remember_moves` is on.
    moves: RememberedMoves,
    automation: Automation,
}

impl std::fmt::Debug for GitGraphApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitGraphApp")
            .field("repo_path", &self.repo_path)
            .finish_non_exhaustive()
    }
}

impl GitGraphApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        repo_path: PathBuf,
        repo: Repo,
        overrides: impl FnOnce(&mut Settings),
        automation: Automation,
    ) -> GitGraphApp {
        let persist = !automation.is_active();
        let mut settings: Settings = cc
            .storage
            .filter(|_| persist)
            .and_then(|s| eframe::get_value(s, STORAGE_KEY))
            .unwrap_or_default();
        overrides(&mut settings);
        let moves: RememberedMoves = cc
            .storage
            .filter(|_| persist)
            .map(load_moves)
            .unwrap_or_default();
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        GitGraphApp {
            repo_path,
            repo: Arc::new(repo),
            settings,
            persist,
            scene: None,
            requested: None,
            job: None,
            view: View::default(),
            needs_initial_view: true,
            canvas: Rect::NOTHING,
            hovered: None,
            hovered_edge: None,
            selection: Selection::default(),
            preview: None,
            context_node: None,
            pending_select: Vec::new(),
            drag: None,
            search: Search::default(),
            status: None,
            show_shortcuts: false,
            show_legend: false,
            show_branch_colors: false,
            show_about: false,
            export_path: None,
            messages: Messages::default(),
            moves,
            automation,
        }
    }

    /// Starts a new layout when the graph or layout options changed, and installs finished
    /// layouts. Layout runs on a worker thread; the previous scene stays visible meanwhile.
    fn ensure_scene(&mut self, ctx: &egui::Context) {
        let key = (self.settings.graph.clone(), self.settings.layout.clone());
        if self.requested.as_ref() != Some(&key) {
            self.requested = Some(key);
            let font = FontId::monospace(FONT_SIZE);
            let text_height = ctx.fonts_mut(|f| f.row_height(&font));
            let input = ctx.fonts_mut(|f| {
                let mut width = |s: &str| {
                    f.layout_no_wrap(s.to_owned(), font.clone(), Color32::WHITE)
                        .size()
                        .x
                };
                Scene::prepare(&self.repo, &self.settings, &mut width, text_height)
            });
            let (tx, rx) = std::sync::mpsc::channel();
            let repaint = ctx.clone();
            std::thread::spawn(move || {
                // The receiver is gone if a newer layout superseded this one.
                let _ = tx.send(input.lay_out());
                repaint.request_repaint();
            });
            let pending = std::mem::take(&mut self.pending_select);
            self.job = Some(LayoutJob {
                rx,
                anchor: self.view_anchor(),
                selected_commits: if pending.is_empty() {
                    self.selected_commits()
                } else {
                    pending
                },
            });
        }

        let Some(job) = &self.job else { return };
        let Ok(scene) = job.rx.try_recv() else {
            return;
        };
        let job = self.job.take().expect("job exists");
        self.scene = Some(scene);
        self.hovered = None;
        self.hovered_edge = None;
        self.context_node = None;
        self.drag = None;
        self.preview = None;
        let selected: Vec<usize> = job
            .selected_commits
            .iter()
            .filter_map(|oid| self.node_for(oid))
            .collect();
        self.selection.set(None);
        self.selection.extend(selected);
        self.update_search();
        self.restore_moves();
        if let (Some((oid, screen)), Some(scene)) = (job.anchor, &self.scene)
            && let Some(node) = self.node_for(&oid)
            && self.canvas.is_positive()
        {
            let world = scene.node_center(node);
            let fraction = (screen - self.canvas.min) / self.canvas.size();
            self.view.show_at(self.canvas, world, fraction);
        }
    }

    /// The node that shows commit `oid` in the current scene: the commit itself, or the node
    /// it is collapsed into.
    fn node_for(&self, oid: &Oid) -> Option<usize> {
        let scene = self.scene.as_ref()?;
        let commit = scene.repo.lookup(oid)?;
        scene.graph.represented_by(commit).map(|n| n as usize)
    }

    fn repo_key(&self) -> String {
        self.repo.path.display().to_string()
    }

    /// Puts remembered nodes back where they were in a freshly laid-out scene.
    fn restore_moves(&mut self) {
        if !self.settings.remember_moves {
            return;
        }
        let Some(moves) = self.moves.get(&self.repo_key()) else {
            return;
        };
        let Some(scene) = &mut self.scene else { return };
        let saved: Vec<_> = moves
            .iter()
            .filter_map(|(hex, &(dx, dy, by_hand))| {
                let node = Oid::from_hex(hex)
                    .and_then(|oid| scene.repo.lookup(&oid))
                    .and_then(|c| scene.graph.node_of(c))?;
                Some((
                    node as usize,
                    gitgraph_core::layout::Point::new(dx, dy),
                    by_hand,
                ))
            })
            .collect();
        scene.net.restore(saved);
    }

    /// Records where the current scene's nodes rest, for this repository.
    fn record_moves(&mut self) {
        if !self.settings.remember_moves {
            return;
        }
        let Some(scene) = &self.scene else { return };
        let offsets: std::collections::HashMap<String, (f32, f32, bool)> = scene
            .net
            .rest_offsets()
            .map(|(node, d, by_hand)| {
                (
                    scene
                        .repo
                        .commit(scene.graph.nodes[node].commit)
                        .oid
                        .to_hex(),
                    (d.x, d.y, by_hand),
                )
            })
            .collect();
        let key = self.repo_key();
        if offsets.is_empty() {
            self.moves.remove(&key);
        } else {
            self.moves.insert(key, offsets);
        }
    }

    pub fn is_laying_out(&self) -> bool {
        self.job.is_some()
    }

    /// A commit near the middle of the view, with its screen position, for keeping the view
    /// stable across rebuilds.
    fn view_anchor(&self) -> Option<(Oid, Pos2)> {
        let scene = self.scene.as_ref()?;
        if !self.canvas.is_positive() {
            return None;
        }
        let node = self.selection.current().or_else(|| {
            let centre = self.view.to_world(self.canvas, self.canvas.center());
            (0..scene.node_count()).min_by(|&a, &b| {
                let da = scene.node_center(a).distance_sq(centre);
                let db = scene.node_center(b).distance_sq(centre);
                da.total_cmp(&db)
            })
        })?;
        let oid = scene.repo.commit(scene.graph.nodes[node].commit).oid;
        Some((
            oid,
            self.view.to_screen(self.canvas, scene.node_center(node)),
        ))
    }

    fn selected_commit(&self) -> Option<Oid> {
        let scene = self.scene.as_ref()?;
        Some(
            scene
                .repo
                .commit(scene.graph.nodes[self.selection.current()?].commit)
                .oid,
        )
    }

    /// Commits of the selected nodes, the current node last.
    fn selected_commits(&self) -> Vec<Oid> {
        let Some(scene) = &self.scene else {
            return Vec::new();
        };
        self.selection
            .nodes
            .iter()
            .map(|&n| scene.repo.commit(scene.graph.nodes[n].commit).oid)
            .collect()
    }

    fn reload(&mut self) {
        match gitgraph_core::git::load_repo(&self.repo_path) {
            Ok(repo) => {
                // The scene on screen keeps its own snapshot until the new layout replaces it;
                // the selection is carried over by commit id.
                self.pending_select = self.selected_commits();
                self.repo = Arc::new(repo);
                self.requested = None;
                self.status = Some(("Reloaded".into(), false));
            }
            Err(e) => self.status = Some((format!("Reload failed: {e}"), true)),
        }
    }

    fn update_search(&mut self) {
        self.search.hits.clear();
        self.search.current = None;
        let q = self.search.query.trim().to_lowercase();
        let Some(scene) = &self.scene else { return };
        if q.is_empty() {
            return;
        }
        for (i, node) in scene.graph.nodes.iter().enumerate() {
            let commit = scene.repo.commit(node.commit);
            let matches = commit.oid.to_hex().starts_with(&q)
                || node
                    .refs
                    .iter()
                    .any(|&r| scene.repo.refs[r].name.to_lowercase().contains(&q))
                || commit.subject.to_lowercase().contains(&q)
                || commit.author_name.to_lowercase().contains(&q);
            if matches {
                self.search.hits.push(i);
            }
        }
    }

    fn goto_search_hit(&mut self, forward: bool) {
        let n = self.search.hits.len();
        if n == 0 {
            return;
        }
        let next = match self.search.current {
            None => 0,
            Some(c) if forward => (c + 1) % n,
            Some(c) => (c + n - 1) % n,
        };
        self.search.current = Some(next);
        let node = self.search.hits[next];
        self.selection.set(Some(node));
        self.center_on(node);
    }

    fn center_on(&mut self, node: usize) {
        if let Some(scene) = &self.scene {
            self.view
                .show_at(self.canvas, scene.node_center(node), vec2(0.5, 0.4));
        }
    }

    fn go_to_head(&mut self) {
        if let Some(head) = self.scene.as_ref().and_then(Scene::head_node) {
            self.view.zoom = self.view.zoom.max(0.6);
            // TortoiseGit scrolls HEAD to the top of the window after loading.
            let (world, fraction) = {
                let scene = self.scene.as_ref().unwrap();
                let fraction = match self.settings.layout.direction {
                    Direction::NewestTop => vec2(0.5, 0.12),
                    Direction::NewestBottom => vec2(0.5, 0.88),
                    Direction::NewestLeft => vec2(0.12, 0.5),
                    Direction::NewestRight => vec2(0.88, 0.5),
                };
                (scene.node_center(head), fraction)
            };
            self.view.show_at(self.canvas, world, fraction);
        } else {
            self.fit();
        }
    }

    fn fit(&mut self) {
        if let Some(scene) = &self.scene {
            self.view.fit(self.canvas, scene.bounds(), 1.0);
        }
    }

    /// Changes the arrangement of the nodes (reset, undo, …) and remembers the result. Not
    /// while nodes are being dragged.
    fn rearrange(&mut self, change: impl FnOnce(&mut gitgraph_core::physics::Net)) {
        if matches!(self.drag, Some(Drag::Node { .. })) {
            return;
        }
        if let Some(scene) = &mut self.scene {
            change(&mut scene.net);
        }
        self.record_moves();
    }

    fn reset_positions(&mut self) {
        self.rearrange(|net| net.reset());
    }

    fn undo(&mut self) {
        self.rearrange(|net| {
            net.undo();
        });
    }

    fn redo(&mut self) {
        self.rearrange(|net| {
            net.redo();
        });
    }

    fn return_to_layout(&mut self, nodes: &[usize]) {
        self.rearrange(|net| net.return_to_layout(nodes));
    }

    /// Selects the nodes growing out of `roots` (see [`DragModel::Subtree`]).
    fn select_subtree(&mut self, roots: &[usize]) {
        if let Some(scene) = &self.scene {
            self.selection.extend(scene.graph.subtree(roots));
        }
    }

    /// The selected nodes that rest away from the layout.
    fn displaced_selection(&self) -> Vec<usize> {
        let Some(scene) = &self.scene else {
            return Vec::new();
        };
        self.selection
            .nodes
            .iter()
            .copied()
            .filter(|&n| scene.net.is_displaced(n))
            .collect()
    }

    fn set_drag_model(&mut self, model: DragModel) {
        self.settings.net.model = model;
        self.status = Some((format!("Drag: {}", model.description()), false));
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                ctx.memory_mut(|m| m.surrender_focus(egui::Id::new("search")));
            }
            return;
        }
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k) && !i.modifiers.command);
        let command = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, k));
        if command(Key::F) {
            self.search.request_focus = true;
        }
        if command(Key::C) {
            self.copy_selected_hash(ctx);
        }
        // Most specific first: Ctrl+Z also matches Ctrl+Shift+Z.
        let redo = ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z));
        if redo || command(Key::Y) {
            self.redo();
        }
        if command(Key::Z) {
            self.undo();
        }
        for (key, model) in [Key::Num1, Key::Num2, Key::Num3]
            .into_iter()
            .zip(DragModel::ALL)
        {
            if pressed(key) {
                self.set_drag_model(model);
            }
        }
        if command(Key::Num0) || pressed(Key::Num0) {
            self.view
                .zoom_around(self.canvas, self.canvas.center(), 1.0 / self.view.zoom);
        }
        if pressed(Key::F5) {
            self.reload();
        }
        if pressed(Key::F) {
            self.fit();
        }
        if pressed(Key::Home) || pressed(Key::H) {
            self.go_to_head();
        }
        if pressed(Key::R) {
            self.reset_positions();
        }
        if pressed(Key::Plus) || pressed(Key::Equals) || command(Key::Plus) || command(Key::Equals)
        {
            self.view
                .zoom_around(self.canvas, self.canvas.center(), 1.0 / 0.8);
        }
        if pressed(Key::Minus) || command(Key::Minus) {
            self.view
                .zoom_around(self.canvas, self.canvas.center(), 0.8);
        }
        if pressed(Key::Escape) {
            self.selection.set(None);
        }
        if pressed(Key::F3) || pressed(Key::N) {
            let back = ctx.input(|i| i.modifiers.shift);
            self.goto_search_hit(!back);
        }
        let step = 60.0;
        let page = self.canvas.height() * 0.8;
        let pan = [
            (Key::ArrowLeft, vec2(step, 0.0)),
            (Key::ArrowRight, vec2(-step, 0.0)),
            (Key::ArrowUp, vec2(0.0, step)),
            (Key::ArrowDown, vec2(0.0, -step)),
            (Key::PageUp, vec2(0.0, page)),
            (Key::PageDown, vec2(0.0, -page)),
        ];
        for (key, delta) in pan {
            if pressed(key) {
                self.view.pan_screen(delta);
            }
        }
    }

    fn copy_selected_hash(&self, ctx: &egui::Context) {
        if let Some(oid) = self.selected_commit() {
            ctx.copy_text(oid.to_hex());
        }
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui
                    .add(egui::Button::new("Reload").shortcut_text("F5"))
                    .clicked()
                {
                    self.reload();
                    ui.close();
                }
                if ui.button("Export as SVG…").clicked() {
                    let default = std::env::current_dir()
                        .unwrap_or_default()
                        .join(format!("{}-gitgraph.svg", self.repo.display_name()));
                    self.export_path = Some(default.display().to_string());
                    ui.close();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("View", |ui| {
                if ui
                    .add(egui::Button::new("Zoom in").shortcut_text("+"))
                    .clicked()
                {
                    self.view
                        .zoom_around(self.canvas, self.canvas.center(), 1.0 / 0.8);
                }
                if ui
                    .add(egui::Button::new("Zoom out").shortcut_text("-"))
                    .clicked()
                {
                    self.view
                        .zoom_around(self.canvas, self.canvas.center(), 0.8);
                }
                if ui
                    .add(egui::Button::new("Zoom to 100%").shortcut_text("0"))
                    .clicked()
                {
                    self.view
                        .zoom_around(self.canvas, self.canvas.center(), 1.0 / self.view.zoom);
                }
                if ui
                    .add(egui::Button::new("Fit graph").shortcut_text("F"))
                    .clicked()
                {
                    self.fit();
                    ui.close();
                }
                if ui
                    .add(egui::Button::new("Go to HEAD").shortcut_text("Home"))
                    .clicked()
                {
                    self.go_to_head();
                    ui.close();
                }
                ui.separator();
                ui.checkbox(&mut self.settings.show_overview, "Show overview");
                ui.checkbox(
                    &mut self.settings.show_hidden_counts,
                    "Show collapsed-commit counts",
                );
                ui.checkbox(
                    &mut self.settings.highlight_edges,
                    "Highlight edges of selection",
                );
                ui.separator();
                ui.menu_button("Look", |ui| {
                    for look in Look::ALL {
                        if ui
                            .radio(Look::of(&self.settings) == Some(look), look.label())
                            .clicked()
                        {
                            look.apply(&mut self.settings);
                        }
                    }
                });
                ui.menu_button("Edges", |ui| {
                    for s in EdgeStyle::ALL {
                        ui.radio_value(&mut self.settings.edge_style, s, s.label());
                    }
                });
                ui.menu_button("Arrows", |ui| {
                    for a in Arrows::ALL {
                        ui.radio_value(&mut self.settings.arrows, a, a.label());
                    }
                });
                ui.menu_button("Theme", |ui| {
                    for t in ThemeChoice::ALL {
                        ui.radio_value(&mut self.settings.theme, t, t.label());
                    }
                });
                if ui.button("Branch colours…").clicked() {
                    self.show_branch_colors = true;
                    ui.close();
                }
            });
            // egui closes menus on any click inside them, which would close this one when you
            // click into its text fields. It closes on clicks outside instead, so its options
            // can also be changed several at a time.
            egui::containers::menu::MenuButton::new("Graph")
                .config(
                    egui::containers::menu::MenuConfig::new()
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
                )
                .ui(ui, |ui| self.graph_menu(ui));
            ui.menu_button("Drag", |ui| self.drag_menu(ui));
            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard and mouse").clicked() {
                    self.show_shortcuts = true;
                    ui.close();
                }
                if ui.button("Legend").clicked() {
                    self.show_legend = true;
                    ui.close();
                }
                ui.separator();
                if ui.button("About gitgraph").clicked() {
                    self.show_about = true;
                    ui.close();
                }
                ui.label(RichText::new(format!("gitgraph {}", crate::VERSION)).weak());
            });
        });
    }

    fn graph_menu(&mut self, ui: &mut Ui) {
        let g = &mut self.settings.graph;
        ui.label(RichText::new("Show").weak());
        for s in Simplification::ALL {
            ui.radio_value(&mut g.simplification, s, s.label());
        }
        ui.separator();
        ui.checkbox(&mut g.show_local_branches, "Local branches");
        ui.checkbox(&mut g.show_remote_branches, "Remote branches");
        ui.checkbox(&mut g.show_tags, "Tags");
        ui.add_enabled(g.show_tags, egui::Checkbox::new(&mut g.tags_make_nodes, "Show all tags"))
            .on_hover_text("When off, a tag alone does not make a commit a node (TortoiseGit's \"Show all tags\").");
        ui.checkbox(&mut g.show_stash, "Stash");
        ui.checkbox(&mut g.show_other_refs, "Other refs")
            .on_hover_text(
                "Refs outside heads, remotes and tags, e.g. refs/pull/* or tool checkpoints.",
            );
        ui.separator();
        ui.checkbox(&mut g.current_branch_only, "Current branch only")
            .on_hover_text("Only HEAD's history (TortoiseGit filter \"Current branch\").");
        ui.horizontal(|ui| {
            ui.label("Branch filter");
            ui.add(egui::TextEdit::singleline(&mut g.ref_filter).hint_text("e.g. main, release").desired_width(160.0))
                .on_hover_text("Only branches and tags whose names contain one of these comma-separated words start history.");
        });
        ui.horizontal(|ui| {
            ui.label("Hide branches");
            ui.add(egui::TextEdit::singleline(&mut g.hide_branches).hint_text("e.g. pipeline/*, release/*").desired_width(160.0))
                .on_hover_text("Leave out branches matching these comma-separated wildcards (* is any text, ? one character; origin/release/1 matches release/*), with the history only they lead to. Branches that a shown branch's history contains stay, and so does the current branch.");
        });
        ui.checkbox(&mut g.first_parent_only, "First parent only")
            .on_hover_text(
                "Follow only first parents: merged side branches without refs disappear.",
            );
        ui.separator();
        let l = &mut self.settings.layout;
        ui.menu_button("Direction", |ui| {
            for d in Direction::ALL {
                ui.radio_value(&mut l.direction, d, d.label());
            }
        });
        ui.menu_button("Vertical placement", |ui| {
            for r in Ranking::ALL {
                ui.radio_value(&mut l.ranking, r, r.label());
            }
        });
        ui.checkbox(&mut l.concentrate_edges, "Bundle edges into trunks")
            .on_hover_text(
                "Edges running into the same commit share one line where they run in parallel.",
            );
        ui.menu_button("Spacing", |ui| {
            ui.add(egui::Slider::new(&mut l.layer_gap, 10.0..=120.0).text("between layers"));
            ui.add(egui::Slider::new(&mut l.gap_per_span, 0.0..=0.5).text("extra for slanted edges"))
                .on_hover_text("Widen gaps that long sideways edges cross, so edges stay steep (TortoiseGit does this, up to 300).");
            ui.add(egui::Slider::new(&mut l.node_gap, 5.0..=100.0).text("between nodes"));
            ui.add(egui::Slider::new(&mut l.edge_gap, 2.0..=40.0).text("between edges"));
            ui.add(egui::Slider::new(&mut l.max_layer_width, 0.0..=10000.0).text("max row width"))
                .on_hover_text(
                    "Rows wider than this are split so siblings stack up. 0 = never (TortoiseGit).",
                );
            if ui.button("TortoiseGit defaults").clicked() {
                *l = LayoutOptions {
                    direction: l.direction,
                    ranking: l.ranking,
                    ..LayoutOptions::default()
                };
            }
        });
    }

    fn drag_menu(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("What moves when you drag").weak());
        for (m, key) in DragModel::ALL.into_iter().zip(["1", "2", "3"]) {
            let text = format!("{} ({key})", m.label());
            if ui
                .radio(self.settings.net.model == m, text)
                .on_hover_text(m.description())
                .clicked()
            {
                self.set_drag_model(m);
            }
        }
        ui.separator();
        let n = &mut self.settings.net;
        ui.add_enabled_ui(n.model.adapts(), |ui| {
            ui.add(egui::Slider::new(&mut n.pull, 0.0..=1.0).text("pull"))
                .on_hover_text("How far neighbours are pulled along their edges");
            ui.add(egui::Slider::new(&mut n.push, 0.0..=1.0).text("push"))
                .on_hover_text("How strongly, and from how far, nodes push each other away");
            ui.add(egui::Slider::new(&mut n.wobble, 0.0..=1.0).text("wobble"));
            ui.checkbox(&mut n.avoid_overlap, "Keep nodes from overlapping");
        });
        let before = self.settings.remember_moves;
        ui.checkbox(&mut self.settings.remember_moves, "Remember moved nodes")
            .on_hover_text(
                "Keep nodes where you moved them, per repository, across runs and relayouts.",
            );
        if self.settings.remember_moves && !before {
            self.record_moves();
        }
        ui.separator();
        let (can_undo, can_redo) = self
            .scene
            .as_ref()
            .map_or((false, false), |s| (s.net.can_undo(), s.net.can_redo()));
        if ui
            .add_enabled(
                can_undo,
                egui::Button::new("Undo move").shortcut_text("Ctrl+Z"),
            )
            .clicked()
        {
            self.undo();
        }
        if ui
            .add_enabled(
                can_redo,
                egui::Button::new("Redo move").shortcut_text("Ctrl+Shift+Z"),
            )
            .clicked()
        {
            self.redo();
        }
        ui.separator();
        if ui
            .add_enabled(
                !self.selection.is_empty(),
                egui::Button::new("Select subtree of selection"),
            )
            .on_hover_text("Add everything that grows out of the selected nodes")
            .clicked()
        {
            let roots = self.selection.nodes.clone();
            self.select_subtree(&roots);
            ui.close();
        }
        let displaced = self.displaced_selection();
        if !self.selection.is_empty()
            && ui
                .add_enabled(
                    !displaced.is_empty(),
                    egui::Button::new("Return selection to layout"),
                )
                .clicked()
        {
            self.return_to_layout(&displaced);
            ui.close();
        }
        if ui
            .add(egui::Button::new("Return all nodes to layout").shortcut_text("R"))
            .clicked()
        {
            self.reset_positions();
            ui.close();
        }
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        // Wraps onto a second line in narrow windows.
        ui.horizontal_wrapped(|ui| {
            let g = &mut self.settings.graph;
            for s in Simplification::ALL {
                ui.selectable_value(&mut g.simplification, s, s.label());
            }
            group_break(ui, 190.0);
            ui.toggle_value(&mut g.show_local_branches, "Local");
            ui.toggle_value(&mut g.show_remote_branches, "Remote");
            ui.toggle_value(&mut g.show_tags, "Tags");
            group_break(ui, 290.0);
            let current = Look::of(&self.settings);
            egui::ComboBox::from_id_salt("look")
                .selected_text(current.map_or("Custom", Look::label))
                .show_ui(ui, |ui| {
                    for look in Look::ALL {
                        if ui
                            .selectable_label(current == Some(look), look.label())
                            .clicked()
                        {
                            look.apply(&mut self.settings);
                        }
                    }
                });
            egui::ComboBox::from_id_salt("direction")
                .selected_text(self.settings.layout.direction.label())
                .show_ui(ui, |ui| {
                    for d in Direction::ALL {
                        ui.selectable_value(&mut self.settings.layout.direction, d, d.label());
                    }
                });
            group_break(ui, 100.0);
            if ui
                .button("Fit")
                .on_hover_text("Fit the whole graph (F)")
                .clicked()
            {
                self.fit();
            }
            if ui
                .button("HEAD")
                .on_hover_text("Go to HEAD (Home)")
                .clicked()
            {
                self.go_to_head();
            }
            group_break(ui, 230.0);
            ui.label("Drag:");
            for (m, key) in DragModel::ALL.into_iter().zip(["1", "2", "3"]) {
                if ui
                    .selectable_label(self.settings.net.model == m, m.label())
                    .on_hover_text(format!("{} ({key})", m.description()))
                    .clicked()
                {
                    self.set_drag_model(m);
                }
            }
            if self.scene.as_ref().is_some_and(|s| s.net.any_displaced())
                && ui
                    .button("Reset")
                    .on_hover_text("Return all nodes to the layout (R)")
                    .clicked()
            {
                self.reset_positions();
            }
            group_break(ui, 210.0);
            let search = egui::TextEdit::singleline(&mut self.search.query)
                .id(egui::Id::new("search"))
                .hint_text("Find (Ctrl+F)")
                .desired_width(200.0);
            let resp = ui.add(search);
            if self.search.request_focus {
                resp.request_focus();
                self.search.request_focus = false;
            }
            if resp.changed() {
                self.update_search();
                if !self.search.hits.is_empty() {
                    self.goto_search_hit(true);
                }
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                let back = ui.input(|i| i.modifiers.shift);
                self.goto_search_hit(!back);
                resp.request_focus();
            }
            if !self.search.query.is_empty() {
                let n = self.search.hits.len();
                let text = match self.search.current {
                    Some(c) => format!("{}/{}", c + 1, n),
                    None => format!("{n} found"),
                };
                ui.label(text);
            }
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if self.is_laying_out() {
                ui.spinner();
                ui.label("Laying out…");
                ui.separator();
            }
            if let Some(scene) = &self.scene {
                if self.selection.len() > 1 {
                    ui.label(format!("{} nodes selected ·", self.selection.len()));
                }
                if let Some(sel) = self.selection.current() {
                    let commit = scene.repo.commit(scene.graph.nodes[sel].commit);
                    ui.monospace(commit.oid.short(10));
                    ui.label(format!(
                        "{} — {}, {}",
                        commit.subject, commit.author_name, commit.author_date
                    ));
                } else {
                    ui.label(scene.repo.path.display().to_string());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("{:.0}%", self.view.zoom * 100.0));
                    ui.separator();
                    let hidden = match scene.graph.hidden_branches {
                        0 => String::new(),
                        1 => " · 1 branch hidden".to_owned(),
                        n => format!(" · {n} branches hidden"),
                    };
                    ui.label(format!(
                        "{} nodes · {} commits{hidden} · layout {} ms",
                        scene.node_count(),
                        scene.graph.visible_commits,
                        (scene.build_time + scene.layout_time).as_millis()
                    ));
                    if let Some((msg, error)) = &self.status {
                        ui.separator();
                        let color = if *error {
                            Color32::RED
                        } else {
                            ui.visuals().weak_text_color()
                        };
                        ui.colored_label(color, msg);
                    }
                });
            }
        });
    }

    fn canvas(&mut self, ui: &mut Ui) {
        let (canvas, response) =
            ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.canvas = canvas;
        if self.needs_initial_view && canvas.is_positive() && self.scene.is_some() {
            self.needs_initial_view = false;
            if self.automation.fit {
                self.fit();
            } else {
                self.view.zoom = 1.0;
                self.go_to_head();
            }
        }
        let Some(scene) = &mut self.scene else { return };

        // Hover.
        let pointer = response.hover_pos();
        self.hovered = pointer.and_then(|p| scene.node_at(self.view.to_world(canvas, p)));
        let view = self.view;
        self.hovered_edge = match (pointer, self.hovered, self.drag) {
            (Some(p), None, None) => render::edge_at(
                scene,
                self.settings.edge_style,
                |w| view.to_screen(canvas, w),
                p,
                5.0,
            ),
            _ => None,
        };

        // Dragging: nodes (with their selection) follow the pointer, the background pans, and
        // Shift- or Ctrl-dragging the background selects.
        let modifiers = ui.input(|i| i.modifiers);
        let extend = modifiers.shift || modifiers.command;
        if response.drag_started() {
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or_default();
            let world = self.view.to_world(canvas, origin);
            let node = scene.node_at(world);
            let middle = response.dragged_by(PointerButton::Middle);
            self.drag = match node {
                Some(n) if !middle => {
                    if !self.selection.contains(n) {
                        if extend {
                            self.selection.add(n);
                        } else {
                            self.selection.set(Some(n));
                        }
                    }
                    let model = self.settings.net.model;
                    let nodes = dragged_with(&self.selection, n);
                    let carried = scene.carried_nodes(&nodes, model);
                    scene.net.grab(n, &nodes, &carried, model.adapts());
                    Some(Drag::Node {
                        grab: world - scene.node_center(n),
                    })
                }
                None if extend && !middle => Some(Drag::Select {
                    start: world,
                    end: world,
                }),
                _ => Some(Drag::Pan),
            };
        }
        if response.dragged() {
            let pointer = response
                .interact_pointer_pos()
                .map(|p| self.view.to_world(canvas, p));
            match &mut self.drag {
                Some(Drag::Node { grab }) => {
                    if let Some(p) = pointer {
                        scene.net.drag_to(to_point(p - *grab));
                    }
                }
                Some(Drag::Select { end, .. }) => *end = pointer.unwrap_or(*end),
                Some(Drag::Pan) | None => self.view.pan_screen(response.drag_delta()),
            }
        }
        let band = match self.drag {
            Some(Drag::Select { start, end }) => Some(Rect::from_two_pos(start, end)),
            _ => None,
        };
        let mut moved = false;
        if response.drag_stopped() {
            match self.drag {
                Some(Drag::Node { .. }) => {
                    scene.net.release(&self.settings.net);
                    moved = true;
                }
                Some(Drag::Select { start, end }) => {
                    self.selection
                        .extend(scene.nodes_in(Rect::from_two_pos(start, end)));
                }
                Some(Drag::Pan) | None => {}
            }
            self.drag = None;
        }

        // Clicks: select a node; Ctrl toggles it, Shift adds it.
        if response.clicked() {
            match self.hovered {
                Some(n) if modifiers.command => self.selection.toggle(n),
                Some(n) if modifiers.shift => self.selection.add(n),
                Some(n) => self.selection.set(Some(n)),
                None if extend => {}
                None => self.selection.set(None),
            }
        }
        if response.secondary_clicked() {
            self.context_node = self.hovered;
            if let Some(n) = self.hovered
                && !self.selection.contains(n)
            {
                self.selection.set(Some(n));
            }
        }
        if response.double_clicked() && self.hovered.is_none() {
            self.view.fit(canvas, scene.bounds(), 1.0);
        }

        // Wheel: scroll; Ctrl+wheel or pinch: zoom around the pointer.
        if response.hovered() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
            if zoom != 1.0 {
                self.view
                    .zoom_around(canvas, pointer.unwrap_or(canvas.center()), zoom);
            } else if scroll != Vec2::ZERO {
                self.view.pan_screen(scroll);
            }
        }

        // Physics.
        let dt = self
            .automation
            .fixed_dt()
            .unwrap_or_else(|| ui.input(|i| i.stable_dt));
        if scene.net.step(dt, &self.settings.net) {
            ui.ctx().request_repaint();
        }

        let palette = palette_for(ui, &self.settings);
        let count = scene.node_count();
        let mut hits = vec![false; count];
        for &h in &self.search.hits {
            hits[h] = true;
        }
        let mut selected = self.selection.mask(count);
        if let Some(band) = band {
            for node in scene.nodes_in(band) {
                selected[node] = true;
            }
        }
        // In Subtree mode, show what a drag would move.
        let mut preview = vec![false; count];
        match (self.hovered, self.drag, self.settings.net.model) {
            (Some(n), None, DragModel::Subtree) => {
                // As a drag would: Shift or Ctrl adds the node to the selection.
                let mut roots = dragged_with(&self.selection, n);
                if extend && !self.selection.contains(n) {
                    roots = self.selection.nodes.clone();
                    roots.push(n);
                }
                if self.preview.as_ref().is_none_or(|(r, _)| *r != roots) {
                    let nodes = scene.carried_nodes(&roots, DragModel::Subtree);
                    self.preview = Some((roots, nodes));
                }
                if let Some((_, nodes)) = &self.preview {
                    for &node in nodes {
                        preview[node] = true;
                    }
                }
            }
            _ => self.preview = None,
        }
        let marks = Marks {
            hovered: self.hovered,
            hovered_edge: self.hovered_edge,
            selected,
            preview,
            search_hits: hits,
        };
        let painter = ui.painter_at(canvas);
        render::paint_scene(
            &painter,
            canvas,
            &self.view,
            scene,
            &palette,
            &self.settings,
            &marks,
        );

        if let Some(band) = band {
            painter.rect(
                self.view.rect_to_screen(canvas, band),
                0.0,
                palette.selection.gamma_multiply(0.12),
                egui::Stroke::new(1.0, palette.selection),
                egui::StrokeKind::Inside,
            );
        }

        if scene.node_count() == 0 {
            painter.text(
                canvas.center(),
                egui::Align2::CENTER_CENTER,
                "No graph available",
                FontId::proportional(16.0),
                palette.edge,
            );
        }

        // Tooltip for the hovered node.
        if let (Some(node), None) = (self.hovered, self.drag) {
            let n = &scene.graph.nodes[node];
            let commit = scene.repo.commit(n.commit);
            let hidden: u32 = scene
                .graph
                .edges
                .iter()
                .filter(|e| e.child as usize == node && e.first_parent)
                .map(|e| e.hidden)
                .sum();
            let refs: Vec<&str> = n
                .refs
                .iter()
                .map(|&r| scene.repo.refs[r].full_name.as_str())
                .collect();
            let messages = &mut self.messages;
            let repo_path = &scene.repo.path;
            let ctx = ui.ctx().clone();
            response.clone().on_hover_ui_at_pointer(|ui| {
                ui.monospace(commit.oid.to_hex());
                ui.label(format!(
                    "{} <{}>  {}",
                    commit.author_name, commit.author_email, commit.author_date
                ));
                ui.add_space(4.0);
                ui.label(RichText::new(&commit.subject).strong());
                match messages.get(repo_path, commit.oid, &ctx) {
                    Some(message) => {
                        let body = message.split_once('\n').map_or("", |(_, b)| b.trim());
                        if !body.is_empty() {
                            // TortoiseGit truncates at 8000 characters.
                            let body: String = body.lines().take(40).collect::<Vec<_>>().join("\n");
                            ui.label(body.chars().take(4000).collect::<String>());
                        }
                    }
                    None => {
                        ui.spinner();
                    }
                }
                if !refs.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new(refs.join("\n")).weak());
                }
                if hidden > 0 {
                    ui.label(RichText::new(format!("{hidden} commits collapsed below")).weak());
                }
            });
        }

        // Tooltip for the hovered edge: the commits collapsed into it.
        if let (Some(e), None) = (self.hovered_edge, self.drag) {
            let edge = scene.graph.edges[e];
            let child = &scene.graph.nodes[edge.child as usize];
            let parent = &scene.graph.nodes[edge.parent as usize];
            let hidden = scene.graph.collapsed_commits(&scene.repo, edge, 12);
            response.clone().on_hover_ui_at_pointer(|ui| {
                let short = |c: CommitIx| scene.repo.commit(c).oid.short(8);
                ui.label(format!(
                    "{} → {}{}",
                    short(child.commit),
                    short(parent.commit),
                    if edge.first_parent {
                        ""
                    } else {
                        "  (merged branch)"
                    }
                ));
                if edge.hidden == 0 {
                    ui.label(RichText::new("direct parent").weak());
                    return;
                }
                ui.label(RichText::new(format!("{} commits collapsed:", edge.hidden)).strong());
                for c in &hidden {
                    let commit = scene.repo.commit(*c);
                    ui.horizontal(|ui| {
                        ui.monospace(commit.oid.short(8));
                        ui.label(&commit.subject);
                    });
                }
                if (edge.hidden as usize) > hidden.len() {
                    ui.label(
                        RichText::new(format!(
                            "… and {} more",
                            edge.hidden as usize - hidden.len()
                        ))
                        .weak(),
                    );
                }
            });
        }

        // Context menu.
        let context_node = self.context_node;
        // A node's menu acts on the selection it belongs to.
        let group: Vec<usize> = match context_node {
            Some(n) if self.selection.contains(n) => self.selection.nodes.clone(),
            Some(n) => vec![n],
            None => Vec::new(),
        };
        let mut action = None;
        response.context_menu(|ui| {
            let Some(node) = context_node else {
                if ui.button("Fit graph").clicked() {
                    action = Some(MenuAction::Fit);
                    ui.close();
                }
                if ui.button("Return all nodes to layout").clicked() {
                    action = Some(MenuAction::ResetAll);
                    ui.close();
                }
                return;
            };
            let n = &scene.graph.nodes[node];
            let commit = scene.repo.commit(n.commit);
            if ui.button("Copy hash").clicked() {
                ui.ctx().copy_text(commit.oid.to_hex());
                ui.close();
            }
            if ui.button("Copy ref names").clicked() {
                let names: Vec<&str> = n
                    .refs
                    .iter()
                    .map(|&r| scene.repo.refs[r].full_name.as_str())
                    .collect();
                let text = if names.is_empty() {
                    commit.oid.to_hex()
                } else {
                    names.join("\n")
                };
                ui.ctx().copy_text(text);
                ui.close();
            }
            if ui.button("Copy subject").clicked() {
                ui.ctx().copy_text(commit.subject.clone());
                ui.close();
            }
            ui.separator();
            if ui
                .button("Select subtree")
                .on_hover_text(
                    "Select everything that grows out of this (first-parent descendants)",
                )
                .clicked()
            {
                action = Some(MenuAction::SelectSubtree(group.clone()));
                ui.close();
            }
            let displaced: Vec<usize> = group
                .iter()
                .copied()
                .filter(|&n| scene.net.is_displaced(n))
                .collect();
            let label = if group.len() > 1 {
                "Return selection to layout"
            } else {
                "Return node to layout"
            };
            if !displaced.is_empty() && ui.button(label).clicked() {
                action = Some(MenuAction::ReturnToLayout(displaced));
                ui.close();
            }
            if ui.button("Centre view here").clicked() {
                action = Some(MenuAction::Center(node));
                ui.close();
            }
        });
        match action {
            Some(MenuAction::Fit) => self.fit(),
            Some(MenuAction::ResetAll) => self.reset_positions(),
            Some(MenuAction::ReturnToLayout(nodes)) => self.return_to_layout(&nodes),
            Some(MenuAction::SelectSubtree(roots)) => self.select_subtree(&roots),
            Some(MenuAction::Center(node)) => self.center_on(node),
            None => {}
        }

        if moved {
            self.record_moves();
        }
        if self.settings.show_overview {
            self.overview(ui, canvas);
        }
    }

    fn overview(&mut self, ui: &mut Ui, canvas: Rect) {
        let Some(scene) = &self.scene else { return };
        // TortoiseGit: max(100, w/4) x max(200, h/4) in the bottom-right corner.
        let size = vec2(
            (canvas.width() / 4.0).max(100.0),
            (canvas.height() / 4.0).max(200.0),
        );
        let rect = Rect::from_min_size(canvas.max - size - vec2(12.0, 12.0), size);
        let palette = palette_for(ui, &self.settings);
        let painter = ui.painter_at(rect.expand(2.0));
        let (world, scale) =
            render::paint_overview(&painter, rect, canvas, &self.view, scene, &palette);
        let resp = ui.interact(rect, egui::Id::new("overview"), Sense::click_and_drag());
        if (resp.clicked() || resp.dragged())
            && let Some(p) = resp.interact_pointer_pos()
        {
            let target = world.center() + (p - rect.center()) / scale;
            self.view.show_at(canvas, target, vec2(0.5, 0.5));
        }
    }

    fn export_window(&mut self, ctx: &egui::Context) {
        let Some(path) = &mut self.export_path else {
            return;
        };
        let mut open = true;
        let mut save = false;
        egui::Window::new("Export as SVG")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("The whole graph is written at 100%, as currently arranged.");
                let resp = ui.add(egui::TextEdit::singleline(path).desired_width(420.0));
                save = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                save |= ui.button("Save").clicked();
            });
        if save {
            let path = PathBuf::from(path.trim());
            let palette = Palette::new(
                ctx.global_style().visuals.dark_mode,
                &self.settings.branch_colors,
            );
            self.status = Some(match &self.scene {
                Some(scene) => match std::fs::write(
                    &path,
                    crate::export::to_svg(scene, &self.settings, &palette),
                ) {
                    Ok(()) => (format!("Saved {}", path.display()), false),
                    Err(e) => (format!("Could not save {}: {e}", path.display()), true),
                },
                None => ("Nothing to export yet".into(), true),
            });
            open = false;
        }
        if !open {
            self.export_path = None;
        }
    }

    fn legend_window(&mut self, ctx: &egui::Context) {
        let palette = Palette::new(
            ctx.global_style().visuals.dark_mode,
            &self.settings.branch_colors,
        );
        egui::Window::new("Legend")
            .open(&mut self.show_legend)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                let swatch = |ui: &mut Ui, fill: Color32, text: &str, what: &str| {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(150.0, 20.0), Sense::hover());
                        ui.painter().rect_filled(rect, 4.0, fill);
                        ui.painter().text(
                            rect.left_center() + vec2(8.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            text,
                            FontId::monospace(12.0),
                            crate::theme::text_on(fill),
                        );
                        ui.label(what);
                    });
                };
                swatch(
                    ui,
                    palette.current_branch,
                    "main",
                    "Current branch (HEAD), or a detached HEAD",
                );
                swatch(ui, palette.local_branch, "feature/x", "Local branch");
                swatch(
                    ui,
                    palette.remote_branch,
                    "origin/feature/x",
                    "Remote-tracking branch",
                );
                swatch(ui, palette.tag, "v1.2.0", "Tag");
                swatch(ui, palette.stash, "stash", "Stash");
                swatch(ui, palette.other_ref, "pull/12/head", "Other ref");
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(150.0, 20.0), Sense::hover());
                    ui.painter().rect_filled(rect, 4.0, palette.plain_fill);
                    ui.painter().text(
                        rect.left_center() + vec2(8.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        "1a2b3c4d",
                        FontId::monospace(12.0),
                        palette.plain_text,
                    );
                    ui.label("Commit without refs (branch point or merge)");
                });
                for rule in &self.settings.branch_colors {
                    swatch(ui, rule.color, &rule.patterns, "Branches matching");
                }
                if ui.link("Branch colours…").clicked() {
                    self.show_branch_colors = true;
                }
                ui.add_space(6.0);
                ui.label(
                    "Arrows point from a commit to its parents. Edges may stand for many hidden",
                );
                ui.label("commits; hover an edge to list them. A blue dot marks a node you moved.");
            });
    }

    fn branch_colors_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("Branch colours")
            .open(&mut self.show_branch_colors)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                // A fixed width, so the text wraps there instead of squeezing the fields.
                ui.set_width(380.0);
                ui.label(
                    "Branches whose names match a rule get its colour. The first matching rule \
                     wins, and the current branch stays red.",
                );
                ui.label(
                    RichText::new(
                        "* is any text, ? one character; commas separate wildcards. \
                         origin/feature/x matches feature/*.",
                    )
                    .weak(),
                );
                ui.add_space(4.0);
                let rules = &mut self.settings.branch_colors;
                let (mut swap, mut remove) = (None, None);
                // Rows rather than a Grid: a Grid keeps text fields at their first-frame width.
                let count = rules.len();
                for (i, rule) in rules.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        egui::color_picker::color_edit_button_srgba(
                            ui,
                            &mut rule.color,
                            egui::color_picker::Alpha::Opaque,
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut rule.patterns)
                                .hint_text("e.g. feature/*")
                                .desired_width(240.0),
                        );
                        if ui
                            .add_enabled(i > 0, egui::Button::new("⏶"))
                            .on_hover_text("Move up")
                            .clicked()
                        {
                            swap = Some(i - 1);
                        }
                        if ui
                            .add_enabled(i + 1 < count, egui::Button::new("⏷"))
                            .on_hover_text("Move down")
                            .clicked()
                        {
                            swap = Some(i);
                        }
                        if ui.button("🗑").on_hover_text("Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = swap {
                    rules.swap(i, i + 1);
                }
                if let Some(i) = remove {
                    rules.remove(i);
                }
                if ui.button("Add rule").clicked() {
                    let suggested = BranchColor::SUGGESTED;
                    rules.push(BranchColor {
                        patterns: String::new(),
                        color: suggested[rules.len() % suggested.len()],
                    });
                }
            });
    }

    fn shortcuts_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("Keyboard and mouse")
            .open(&mut self.show_shortcuts)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                egui::Grid::new("shortcuts").striped(true).show(ui, |ui| {
                    for (keys, what) in [
                        (
                            "Drag a node",
                            "Move it, with the rest of the selection it belongs to",
                        ),
                        (
                            "1 / 2 / 3",
                            "Drag mode Adapt (the graph gives way) / Free (nothing else \
                             moves) / Subtree (take along what grows out of it)",
                        ),
                        ("Click a node", "Select it"),
                        (
                            "Ctrl+click / Shift+click",
                            "Toggle it in / add it to the selection",
                        ),
                        (
                            "Shift+drag the background",
                            "Select the nodes in a rectangle",
                        ),
                        ("Esc", "Clear the selection"),
                        ("Ctrl+Z / Ctrl+Shift+Z", "Undo / redo a move"),
                        ("R", "Return all nodes to the layout"),
                        ("Drag the background", "Pan"),
                        ("Wheel / Shift+wheel", "Scroll vertically / horizontally"),
                        ("Ctrl+wheel, pinch", "Zoom around the pointer"),
                        ("+ / - / 0", "Zoom in / out / 100%"),
                        ("F, double-click background", "Fit the whole graph"),
                        ("Home, H", "Go to HEAD"),
                        ("Ctrl+F", "Find; Enter / Shift+Enter for next / previous"),
                        ("F3, N", "Next search hit"),
                        ("Ctrl+C", "Copy the selected commit's hash"),
                        ("F5", "Reload the repository"),
                        (
                            "Right-click a node",
                            "Copy hash or refs, select its subtree, return it to the layout",
                        ),
                    ] {
                        ui.strong(keys);
                        ui.label(what);
                        ui.end_row();
                    }
                });
            });
    }

    /// The "Appropriate Legal Notices" of GPL-3.0 section 5(d). NOTICE requires works based on
    /// gitgraph to keep showing them.
    fn about_window(&mut self, ctx: &egui::Context) {
        const NOTICE: &str = include_str!("../../../NOTICE");
        const LICENSE: &str = include_str!("../../../LICENSE");
        // Room for the title bar and the heading; the texts scroll within the rest.
        let max_height = ctx.content_rect().height() - 140.0;
        egui::Window::new("About gitgraph")
            .open(&mut self.show_about)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.heading(format!("gitgraph {}", crate::VERSION));
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .max_height(max_height)
                    .show(ui, |ui| {
                        // Both texts are wrapped at 80 columns already.
                        ui.add(egui::Label::new(RichText::new(NOTICE).monospace()).extend());
                        ui.collapsing("GNU General Public License, version 3", |ui| {
                            ui.add(egui::Label::new(RichText::new(LICENSE).monospace()).extend());
                        });
                    });
            });
    }
}

/// Separates groups in the (wrapping) toolbar: a separator, or a new line if the next group,
/// about `width` wide, would not fit.
fn group_break(ui: &mut Ui, width: f32) {
    // (In a wrapping layout `available_width` is the whole row.)
    if ui.max_rect().right() - ui.cursor().min.x < width {
        ui.end_row();
    } else {
        ui.separator();
    }
}

fn palette_for(ui: &Ui, settings: &Settings) -> Palette {
    Palette::new(ui.visuals().dark_mode, &settings.branch_colors)
}

enum MenuAction {
    Fit,
    ResetAll,
    ReturnToLayout(Vec<usize>),
    SelectSubtree(Vec<usize>),
    Center(usize),
}

impl eframe::App for GitGraphApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.set_theme(match self.settings.theme {
            ThemeChoice::System => egui::ThemePreference::System,
            ThemeChoice::Light => egui::ThemePreference::Light,
            ThemeChoice::Dark => egui::ThemePreference::Dark,
        });
        self.ensure_scene(&ctx);
        self.handle_keys(&ctx);

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));
        self.shortcuts_window(&ctx);
        self.legend_window(&ctx);
        self.branch_colors_window(&ctx);
        self.about_window(&ctx);
        self.export_window(&ctx);

        if let Some(scene) = &mut self.scene {
            self.automation
                .drive(&ctx, scene, &mut self.view, self.canvas, &self.settings.net);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.persist {
            eframe::set_value(storage, STORAGE_KEY, &self.settings);
            eframe::set_value(storage, MOVES_KEY, &self.moves);
        }
    }
}
