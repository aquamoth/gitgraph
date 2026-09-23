//! The eframe application: menus, toolbar, canvas interaction and status bar.

use std::path::PathBuf;

use eframe::egui::{
    self, Color32, FontId, Key, Modifiers, PointerButton, Pos2, Rect, RichText, Sense, Ui, Vec2,
    vec2,
};
use gitgraph_core::layout::{Direction, LayoutOptions, Ranking};
use gitgraph_core::physics::DragModel;
use gitgraph_core::revgraph::{GraphOptions, Simplification};
use gitgraph_core::{CommitIx, Repo};

use crate::automation::Automation;
use crate::render::{self, Marks};
use crate::scene::{FONT_SIZE, Scene, to_point};
use crate::settings::{Arrows, EdgeStyle, Look, STORAGE_KEY, Settings};
use crate::theme::{Palette, ThemeChoice};
use crate::view::View;

#[derive(Clone, Copy, Debug)]
enum Drag {
    /// Dragging a node; `grab` is the pointer's offset from the node centre (world units).
    Node {
        grab: Vec2,
    },
    Pan,
}

/// A layout running on a worker thread.
#[derive(Debug)]
struct LayoutJob {
    rx: std::sync::mpsc::Receiver<Scene>,
    /// Commit near the view centre and its screen position, to keep the view steady.
    anchor: Option<(CommitIx, Pos2)>,
    selected_commit: Option<CommitIx>,
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
    repo: Repo,
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
    selected: Option<usize>,
    context_node: Option<usize>,
    /// Commit to select once the scene has been rebuilt (after a reload).
    pending_select: Option<CommitIx>,
    drag: Option<Drag>,
    search: Search,
    status: Option<(String, bool)>,
    show_shortcuts: bool,
    /// Path being edited in the "Export as SVG" dialog, when open.
    export_path: Option<String>,
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
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        GitGraphApp {
            repo_path,
            repo,
            settings,
            persist,
            scene: None,
            requested: None,
            job: None,
            view: View::default(),
            needs_initial_view: true,
            canvas: Rect::NOTHING,
            hovered: None,
            selected: None,
            context_node: None,
            pending_select: None,
            drag: None,
            search: Search::default(),
            status: None,
            show_shortcuts: false,
            export_path: None,
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
            self.job = Some(LayoutJob {
                rx,
                anchor: self.view_anchor(),
                selected_commit: self
                    .pending_select
                    .take()
                    .or_else(|| self.selected_commit()),
            });
        }

        let Some(job) = &self.job else { return };
        let Ok(scene) = job.rx.try_recv() else {
            return;
        };
        let job = self.job.take().expect("job exists");
        self.scene = Some(scene);
        self.hovered = None;
        self.context_node = None;
        self.drag = None;
        self.selected = job.selected_commit.and_then(|c| {
            self.scene
                .as_ref()?
                .graph
                .represented_by(c)
                .map(|n| n as usize)
        });
        self.update_search();
        if let (Some((commit, screen)), Some(scene)) = (job.anchor, &self.scene)
            && let Some(node) = scene.graph.represented_by(commit)
            && self.canvas.is_positive()
        {
            let world = scene.node_center(node as usize);
            let fraction = (screen - self.canvas.min) / self.canvas.size();
            self.view.show_at(self.canvas, world, fraction);
        }
    }

    pub fn is_laying_out(&self) -> bool {
        self.job.is_some()
    }

    /// A commit near the middle of the view, with its screen position, for keeping the view
    /// stable across rebuilds.
    fn view_anchor(&self) -> Option<(CommitIx, Pos2)> {
        let scene = self.scene.as_ref()?;
        if !self.canvas.is_positive() {
            return None;
        }
        let node = self.selected.or_else(|| {
            let centre = self.view.to_world(self.canvas, self.canvas.center());
            (0..scene.node_count()).min_by(|&a, &b| {
                let da = scene.node_center(a).distance_sq(centre);
                let db = scene.node_center(b).distance_sq(centre);
                da.total_cmp(&db)
            })
        })?;
        let commit = scene.graph.nodes[node].commit;
        Some((
            commit,
            self.view.to_screen(self.canvas, scene.node_center(node)),
        ))
    }

    fn selected_commit(&self) -> Option<CommitIx> {
        Some(self.scene.as_ref()?.graph.nodes[self.selected?].commit)
    }

    fn reload(&mut self) {
        match gitgraph_core::git::load_repo(&self.repo_path) {
            Ok(repo) => {
                let selected = self.selected_commit().map(|c| self.repo.commit(c).oid);
                self.repo = repo;
                self.requested = None;
                self.status = Some(("Reloaded".into(), false));
                // Re-select the same commit after the rebuild if it still exists.
                if let Some(oid) = selected {
                    self.pending_select = self.repo.lookup(&oid);
                }
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
            let commit = self.repo.commit(node.commit);
            let matches = commit.oid.to_hex().starts_with(&q)
                || node
                    .refs
                    .iter()
                    .any(|&r| self.repo.refs[r].name.to_lowercase().contains(&q))
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
        self.selected = Some(node);
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

    fn reset_positions(&mut self) {
        if let Some(scene) = &mut self.scene {
            scene.net.reset(&scene.layout);
        }
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
            self.selected = None;
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
        if let Some(c) = self.selected_commit() {
            ctx.copy_text(self.repo.commit(c).oid.to_hex());
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
            });
            ui.menu_button("Graph", |ui| self.graph_menu(ui));
            ui.menu_button("Drag", |ui| self.drag_menu(ui));
            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard and mouse").clicked() {
                    self.show_shortcuts = true;
                    ui.close();
                }
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
        let n = &mut self.settings.net;
        ui.label(RichText::new("When dragging a node").weak());
        for m in DragModel::ALL {
            ui.radio_value(&mut n.model, m, m.label());
        }
        ui.separator();
        ui.add_enabled(
            n.model != DragModel::Rigid,
            egui::Slider::new(&mut n.reach, 0.0..=1.0).text("reach"),
        );
        ui.add_enabled(
            n.model == DragModel::Net,
            egui::Slider::new(&mut n.wobble, 0.0..=1.0).text("wobble"),
        );
        ui.checkbox(&mut n.avoid_overlap, "Push overlapping nodes apart");
        ui.separator();
        if ui
            .add(egui::Button::new("Return all nodes to layout").shortcut_text("R"))
            .clicked()
        {
            self.reset_positions();
            ui.close();
        }
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let g = &mut self.settings.graph;
            for s in Simplification::ALL {
                ui.selectable_value(&mut g.simplification, s, s.label());
            }
            ui.separator();
            ui.toggle_value(&mut g.show_local_branches, "Local");
            ui.toggle_value(&mut g.show_remote_branches, "Remote");
            ui.toggle_value(&mut g.show_tags, "Tags");
            ui.separator();
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
            ui.separator();
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
            if self.scene.as_ref().is_some_and(|s| s.net.any_pinned())
                && ui
                    .button("Unpin all")
                    .on_hover_text("Return dragged nodes to the layout (R)")
                    .clicked()
            {
                self.reset_positions();
            }
            ui.separator();
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
                if let Some(sel) = self.selected {
                    let commit = self.repo.commit(scene.graph.nodes[sel].commit);
                    ui.monospace(commit.oid.short(10));
                    ui.label(format!(
                        "{} — {}, {}",
                        commit.subject, commit.author_name, commit.author_date
                    ));
                } else {
                    ui.label(self.repo.path.display().to_string());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("{:.0}%", self.view.zoom * 100.0));
                    ui.separator();
                    ui.label(format!(
                        "{} nodes · {} commits · layout {} ms",
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

        // Dragging: nodes follow the pointer, the background pans.
        if response.drag_started() {
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or_default();
            let world = self.view.to_world(canvas, origin);
            let node = scene.node_at(world);
            self.drag = match node {
                Some(n) if !response.dragged_by(PointerButton::Middle) => {
                    scene.net.grab(n);
                    Some(Drag::Node {
                        grab: world - scene.node_center(n),
                    })
                }
                _ => Some(Drag::Pan),
            };
        }
        if response.dragged() {
            match self.drag {
                Some(Drag::Node { grab }) => {
                    if let Some(p) = response.interact_pointer_pos() {
                        let target = self.view.to_world(canvas, p) - grab;
                        scene.net.drag_to(to_point(target));
                    }
                }
                Some(Drag::Pan) | None => self.view.pan_screen(response.drag_delta()),
            }
        }
        if response.drag_stopped() {
            if matches!(self.drag, Some(Drag::Node { .. })) {
                scene.net.release();
            }
            self.drag = None;
        }

        // Clicks.
        if response.clicked() {
            self.selected = self.hovered;
        }
        if response.secondary_clicked() {
            self.context_node = self.hovered;
            if self.hovered.is_some() {
                self.selected = self.hovered;
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

        let palette = palette_for(ui);
        let mut hits = vec![false; scene.node_count()];
        for &h in &self.search.hits {
            hits[h] = true;
        }
        let marks = Marks {
            hovered: self.hovered,
            selected: self.selected,
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
            let commit = self.repo.commit(n.commit);
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
                .map(|&r| self.repo.refs[r].full_name.as_str())
                .collect();
            response.clone().on_hover_ui_at_pointer(|ui| {
                ui.monospace(commit.oid.to_hex());
                ui.label(format!(
                    "{} <{}>  {}",
                    commit.author_name, commit.author_email, commit.author_date
                ));
                ui.add_space(4.0);
                ui.label(RichText::new(&commit.subject).strong());
                if !refs.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new(refs.join("\n")).weak());
                }
                if hidden > 0 {
                    ui.label(RichText::new(format!("{hidden} commits collapsed below")).weak());
                }
            });
        }

        // Context menu.
        let context_node = self.context_node;
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
            let commit = self.repo.commit(n.commit);
            if ui.button("Copy hash").clicked() {
                ui.ctx().copy_text(commit.oid.to_hex());
                ui.close();
            }
            if ui.button("Copy ref names").clicked() {
                let names: Vec<&str> = n
                    .refs
                    .iter()
                    .map(|&r| self.repo.refs[r].full_name.as_str())
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
            if scene.net.is_pinned(node) && ui.button("Return node to layout").clicked() {
                action = Some(MenuAction::Unpin(node));
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
            Some(MenuAction::Unpin(node)) => {
                if let Some(scene) = &mut self.scene {
                    let home = scene.layout.nodes[node];
                    scene.net.unpin(node, home);
                }
            }
            Some(MenuAction::Center(node)) => self.center_on(node),
            None => {}
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
        let palette = palette_for(ui);
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
            let palette = if ctx.global_style().visuals.dark_mode {
                Palette::dark()
            } else {
                Palette::light()
            };
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
                            "Move it; the graph follows like a web. It stays pinned.",
                        ),
                        ("Drag the background", "Pan"),
                        ("Wheel / Shift+wheel", "Scroll vertically / horizontally"),
                        ("Ctrl+wheel, pinch", "Zoom around the pointer"),
                        ("+ / - / 0", "Zoom in / out / 100%"),
                        ("F, double-click background", "Fit the whole graph"),
                        ("Home, H", "Go to HEAD"),
                        ("Ctrl+F", "Find; Enter / Shift+Enter for next / previous"),
                        ("F3, N", "Next search hit"),
                        ("Ctrl+C", "Copy the selected commit's hash"),
                        ("R", "Return all dragged nodes to the layout"),
                        ("F5", "Reload the repository"),
                        ("Right-click a node", "Copy hash or refs, unpin"),
                    ] {
                        ui.strong(keys);
                        ui.label(what);
                        ui.end_row();
                    }
                });
            });
    }
}

fn palette_for(ui: &Ui) -> Palette {
    if ui.visuals().dark_mode {
        Palette::dark()
    } else {
        Palette::light()
    }
}

enum MenuAction {
    Fit,
    ResetAll,
    Unpin(usize),
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
        self.export_window(&ctx);

        if let Some(scene) = &mut self.scene {
            self.automation
                .drive(&ctx, scene, &mut self.view, self.canvas);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.persist {
            eframe::set_value(storage, STORAGE_KEY, &self.settings);
        }
    }
}
