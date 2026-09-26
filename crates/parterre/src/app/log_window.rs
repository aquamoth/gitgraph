//! The log window: the commits of a log query one per row, the selected commit's details, and
//! the files it changed. A window of its own (an immediate viewport, like the settings), one at
//! a time; Show log on other nodes replaces its contents. Decided in #27, #28 and #29; the
//! prototype is on the branch `prototype/log-window`.
//!
//! The window is handed a [`LogQuery`] and knows nothing about the graph. Its three panes are
//! separate functions that a [`LogLayout`] arranges; the layout is picked in the header or in
//! the settings, and it and the dividers of each layout are saved with the settings.

use std::collections::HashMap;
use std::sync::{Arc, mpsc};

use eframe::egui::text::{LayoutJob, TextFormat, TextWrapping};
use eframe::egui::{
    self, Color32, CornerRadius, CursorIcon, FontId, Galley, Id, Key, Margin, Modifiers, Rangef,
    Rect, Response, RichText, ScrollArea, Sense, Stroke, Ui, UiBuilder, Vec2, pos2, vec2,
};
use parterre_core::changed_files::{
    ChangedFile, FileColumn, FileOrder, FileStatus, filter_and_sort,
};
use parterre_core::glyphs::{self, Glyph};
use parterre_core::log::LogQuery;
use parterre_core::log_layout::LogLayout;
use parterre_core::revgraph::GraphOptions;
use parterre_core::text::{elide_start, find_urls, thousands};
use parterre_core::{CommitIx, GitRef, Oid, Repo};

use super::{Messages, ParterreApp};
use crate::settings::LogWindowSettings;
use crate::theme::{Palette, text_on};
use crate::widgets;

/// Height of a commit row and of a changed-file row.
const ROW: f32 = 24.0;
const FILE_ROW: f32 = 23.0;
/// Height of a table's column headings.
const HEADING: f32 = 26.0;
/// Thickness of the draggable dividers between panes.
const DIVIDER: f32 = 6.0;
const AUTHOR_WIDTH: f32 = 170.0;
const DATE_WIDTH: f32 = 128.0;
/// A commit list narrower than this (beside another pane) gets narrower author and date
/// columns, as in the prototype's layouts B and D.
const NARROW_LIST: f32 = 720.0;
const NARROW_AUTHOR_WIDTH: f32 = 128.0;
const NARROW_DATE_WIDTH: f32 = 118.0;
const CELL_PAD: f32 = 8.0;
/// How long a Copy button says "Copied".
const COPIED_SECONDS: f64 = 1.2;

fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("log")
}

/// The icon of a layout in the pickers: the arrangement of its panes.
pub fn layout_glyph(layout: LogLayout) -> Glyph {
    match layout {
        LogLayout::Stacked => glyphs::LAYOUT_STACKED,
        LogLayout::SideBySide => glyphs::LAYOUT_SIDE_BY_SIDE,
        LogLayout::DetailsBelow => glyphs::LAYOUT_DETAILS_BELOW,
        LogLayout::FilesRight => glyphs::LAYOUT_FILES_RIGHT,
    }
}

/// The log window's state that outlives its contents: the changed files' sort and filter
/// (kept as you move between commits and logs), and caches. The layout and its dividers are
/// in the settings ([`LogWindowSettings`]).
#[derive(Debug, Default)]
pub struct LogWindow {
    /// What the window shows; `None` while it is closed.
    view: Option<LogView>,
    order: FileOrder,
    filter: String,
    files: ChangedFiles,
    /// The size the window opened with. The viewport builder must not change while the window
    /// is open, or egui would resize it.
    size: Vec2,
    /// The theme last given to the window's title bar.
    title_theme: Option<egui::SystemTheme>,
    /// What was copied last, and when (for the "Copied" feedback).
    copied: Option<(Copied, f64)>,
    /// How many logs were opened, to give each its own scroll positions.
    opened: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Copied {
    Hash,
    Email,
}

/// The commits of one log query.
#[derive(Debug)]
struct LogView {
    /// Tells logs apart, so that a new one starts scrolled to the top. Kept by F5.
    id: u64,
    repo: Arc<Repo>,
    /// [`Repo::refs_by_commit`] of `repo`.
    refs: Vec<Vec<usize>>,
    query: LogQuery,
    commits: Vec<CommitIx>,
    /// Index into `commits`.
    selected: Option<usize>,
    /// Scroll the list to the selected row in the next frame.
    reveal: bool,
    /// The list's scroll offset and height in the last frame, for keeping the selection in
    /// view and for paging.
    scroll: f32,
    list_height: f32,
}

impl LogView {
    fn new(id: u64, repo: Arc<Repo>, query: LogQuery) -> LogView {
        let commits = query.run(&repo);
        LogView {
            id,
            refs: repo.refs_by_commit(),
            repo,
            query,
            selected: (!commits.is_empty()).then_some(0),
            commits,
            reveal: true,
            scroll: 0.0,
            list_height: 0.0,
        }
    }

    fn selected_commit(&self) -> Option<CommitIx> {
        self.commits.get(self.selected?).copied()
    }

    /// Re-runs the query on a newly loaded snapshot, keeping the selected commit if it is still
    /// listed. Commits of the query that are gone from the snapshot are dropped from it.
    fn reload(&mut self, repo: Arc<Repo>) {
        let selected = self.selected_commit().map(|c| self.repo.commit(c).oid);
        let map = |commits: &[CommitIx]| -> Vec<CommitIx> {
            commits
                .iter()
                .filter_map(|&c| repo.lookup(&self.repo.commit(c).oid))
                .collect()
        };
        let mut query = self.query.clone();
        query.tips = map(&self.query.tips);
        query.exclude = map(&self.query.exclude);
        let (scroll, height) = (self.scroll, self.list_height);
        *self = LogView::new(self.id, repo, query);
        (self.scroll, self.list_height) = (scroll, height);
        if let Some(oid) = selected {
            let at = self.repo.lookup(&oid);
            if let Some(i) = self.commits.iter().position(|&c| Some(c) == at) {
                self.selected = Some(i);
            }
        }
    }
}

/// A commit's changed files, or why they could not be listed.
type Listing = Result<Vec<ChangedFile>, String>;

/// Changed files per commit, listed by git on a worker thread when first needed.
#[derive(Debug, Default)]
struct ChangedFiles {
    /// `None` while git works on it.
    cache: HashMap<Oid, Option<Listing>>,
    /// Results; `None` for a request dropped because a newer one came in.
    rx: Option<mpsc::Receiver<(Oid, Option<Listing>)>>,
    tx: Option<mpsc::Sender<Oid>>,
}

impl ChangedFiles {
    /// The changed files of `oid` if they are known; otherwise asks git for them.
    fn get(
        &mut self,
        repo_path: &std::path::Path,
        oid: Oid,
        ctx: &egui::Context,
    ) -> Option<&Listing> {
        while let Some(Ok((oid, files))) = self.rx.as_ref().map(|rx| rx.try_recv()) {
            match files {
                Some(files) => self.cache.insert(oid, Some(files)),
                None => self.cache.remove(&oid),
            };
        }
        if let std::collections::hash_map::Entry::Vacant(slot) = self.cache.entry(oid) {
            slot.insert(None);
            let tx = self.tx.get_or_insert_with(|| {
                let (req_tx, req_rx) = mpsc::channel::<Oid>();
                let (res_tx, res_rx) = mpsc::channel();
                let git = parterre_core::git::Git::new(repo_path);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    while let Ok(mut oid) = req_rx.recv() {
                        // Holding an arrow key asks for one commit after another; only the
                        // newest request still matters. The others are forgotten, so they are
                        // asked for again if they show up once more.
                        while let Ok(newer) = req_rx.try_recv() {
                            if res_tx.send((oid, None)).is_err() {
                                return;
                            }
                            oid = newer;
                        }
                        let files = git.changed_files(&oid).map_err(|e| e.to_string());
                        if res_tx.send((oid, Some(files))).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                    }
                });
                self.rx = Some(res_rx);
                req_tx
            });
            let _ = tx.send(oid);
        }
        self.cache.get(&oid).and_then(Option::as_ref)
    }
}

/// The three panes a layout arranges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pane {
    Commits,
    Details,
    Files,
}

/// What the panes need from the app besides the log window's own state.
struct Env<'a> {
    messages: &'a mut Messages,
    palette: Palette,
    graph: &'a GraphOptions,
    /// The layout and the dividers.
    settings: &'a mut LogWindowSettings,
}

/// Where a layout puts the panes and dividers in the window body.
#[derive(Clone, Copy, Debug)]
struct Arrangement {
    /// Commits, details, changed files.
    panes: [Rect; 3],
    bars: [Bar; 2],
}

/// A draggable divider, and how a pointer position turns into its fraction in
/// [`Dividers`](parterre_core::log_layout::Dividers).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bar {
    rect: Rect,
    /// Dragged sideways: a vertical bar between panes side by side.
    vertical: bool,
    /// Where the bar's middle is (along x for a vertical bar, else y) at fraction 0, and the
    /// room its fraction is of.
    origin: f32,
    room: f32,
}

impl Bar {
    /// The fraction that puts the bar's middle at `at`.
    fn fraction(&self, at: f32) -> f32 {
        (at - self.origin) / self.room
    }
}

/// A range cut in two by a divider at `fraction` of the room the two parts share.
struct Cut {
    first: Rangef,
    bar: Rangef,
    second: Rangef,
    origin: f32,
    room: f32,
}

fn cut(range: Rangef, fraction: f32) -> Cut {
    let room = (range.span() - DIVIDER).max(1.0);
    let at = range.min + (room * fraction).round();
    Cut {
        first: Rangef::new(range.min, at),
        bar: Rangef::new(at, at + DIVIDER),
        second: Rangef::new(at + DIVIDER, range.max.max(at + DIVIDER)),
        origin: range.min + DIVIDER / 2.0,
        room,
    }
}

/// Where `layout`, with its dividers at `fractions`, puts the panes and dividers in `body`.
fn arrange(layout: LogLayout, [a, b]: [f32; 2], body: Rect) -> Arrangement {
    let rect = Rect::from_x_y_ranges;
    let (xs, ys) = (body.x_range(), body.y_range());
    let bar = |rect: Rect, vertical: bool, c: &Cut| Bar {
        rect,
        vertical,
        origin: c.origin,
        room: c.room,
    };
    match layout {
        LogLayout::Stacked => {
            // Both dividers share the height, so the files' share is what the others leave.
            let room = (body.height() - 2.0 * DIVIDER).max(1.0);
            let y1 = body.top() + (room * a).round();
            let y2 = y1 + DIVIDER + (room * b).round();
            let stacked_bar = |y: f32, origin: f32| Bar {
                rect: rect(xs, Rangef::new(y, y + DIVIDER)),
                vertical: false,
                origin,
                room,
            };
            Arrangement {
                panes: [
                    rect(xs, Rangef::new(body.top(), y1)),
                    rect(xs, Rangef::new(y1 + DIVIDER, y2)),
                    rect(
                        xs,
                        Rangef::new(y2 + DIVIDER, body.bottom().max(y2 + DIVIDER)),
                    ),
                ],
                bars: [
                    stacked_bar(y1, body.top() + DIVIDER / 2.0),
                    stacked_bar(y2, body.top() + 1.5 * DIVIDER),
                ],
            }
        }
        LogLayout::SideBySide => {
            let across = cut(xs, a);
            let right = cut(ys, b);
            Arrangement {
                panes: [
                    rect(across.first, ys),
                    rect(across.second, right.first),
                    rect(across.second, right.second),
                ],
                bars: [
                    bar(rect(across.bar, ys), true, &across),
                    bar(rect(across.second, right.bar), false, &right),
                ],
            }
        }
        LogLayout::DetailsBelow => {
            let down = cut(ys, a);
            let below = cut(xs, b);
            Arrangement {
                panes: [
                    rect(xs, down.first),
                    rect(below.first, down.second),
                    rect(below.second, down.second),
                ],
                bars: [
                    bar(rect(xs, down.bar), false, &down),
                    bar(rect(below.bar, down.second), true, &below),
                ],
            }
        }
        LogLayout::FilesRight => {
            let across = cut(xs, a);
            let left = cut(ys, b);
            Arrangement {
                panes: [
                    rect(across.first, left.first),
                    rect(across.first, left.second),
                    rect(across.second, ys),
                ],
                bars: [
                    bar(rect(across.bar, ys), true, &across),
                    bar(rect(across.first, left.bar), false, &left),
                ],
            }
        }
    }
}

/// Colours of the log window beyond egui's visuals, after the prototype.
struct Colors {
    /// Background of the panes (the chrome around them is the panel colour).
    pane: Color32,
    stripe: Color32,
    hover: Color32,
    line: Color32,
    selected_bg: Color32,
    selected_fg: Color32,
    /// The range label.
    link: Color32,
    added: Color32,
    removed: Color32,
    renamed: Color32,
}

fn colors(ui: &Ui) -> Colors {
    let t = widgets::tones(ui);
    if ui.visuals().dark_mode {
        Colors {
            pane: Color32::from_gray(22),
            stripe: Color32::from_white_alpha(5),
            hover: Color32::from_white_alpha(13),
            line: Color32::from_white_alpha(23),
            selected_bg: t.on_bg,
            selected_fg: Color32::from_rgb(0xcf, 0xe5, 0xff),
            link: t.on_fg,
            added: Color32::from_rgb(0x7b, 0xd8, 0x8f),
            removed: Color32::from_rgb(0xff, 0x8a, 0x80),
            renamed: Color32::from_rgb(0xd1, 0xa5, 0xff),
        }
    } else {
        Colors {
            pane: Color32::WHITE,
            stripe: Color32::from_black_alpha(5),
            hover: Color32::from_black_alpha(11),
            line: Color32::from_black_alpha(26),
            selected_bg: t.on_bg,
            selected_fg: Color32::from_rgb(0x0b, 0x3d, 0x7a),
            link: t.on_fg,
            added: Color32::from_rgb(0x2e, 0x7d, 0x32),
            removed: Color32::from_rgb(0xc6, 0x28, 0x28),
            renamed: Color32::from_rgb(0x6a, 0x1b, 0x9a),
        }
    }
}

impl LogWindow {
    /// Shows `query` on `repo`, in place of what the window showed before.
    fn open(&mut self, repo: Arc<Repo>, query: LogQuery, size: Vec2) {
        if self.view.is_none() {
            self.size = size;
        }
        self.opened += 1;
        self.view = Some(LogView::new(self.opened, repo, query));
    }

    pub fn is_open(&self) -> bool {
        self.view.is_some()
    }

    /// Closes the window and forgets the changed files listed for the repository it showed.
    pub fn close(&mut self) {
        self.view = None;
        self.files = ChangedFiles::default();
    }

    /// After F5: re-runs the query on the new snapshot.
    pub fn reload(&mut self, repo: &Arc<Repo>) {
        if let Some(view) = &mut self.view {
            view.reload(repo.clone());
        }
    }

    /// The window's title: `<repo> – Log`.
    fn title(&self) -> String {
        let name = self
            .view
            .as_ref()
            .map(|v| v.repo.display_name())
            .unwrap_or_default();
        format!("{name} – Log")
    }

    /// Esc closes; the arrow keys, Page Up/Down, Home and End move the selection. Only while
    /// no text field has the keyboard.
    fn handle_keys(&mut self, ui: &Ui) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let Some(view) = &mut self.view else { return };
        let n = view.commits.len();
        let page = ((view.list_height / ROW).floor() as usize)
            .saturating_sub(1)
            .max(1);
        let target = ui.input_mut(|i| {
            let mut key = |k: Key| i.consume_key(Modifiers::NONE, k);
            let current = view.selected.unwrap_or(0);
            if key(Key::ArrowDown) {
                Some(current + 1)
            } else if key(Key::ArrowUp) {
                Some(current.saturating_sub(1))
            } else if key(Key::PageDown) {
                Some(current + page)
            } else if key(Key::PageUp) {
                Some(current.saturating_sub(page))
            } else if key(Key::Home) {
                Some(0)
            } else if key(Key::End) {
                Some(usize::MAX)
            } else {
                None
            }
        });
        if let Some(target) = target
            && n > 0
        {
            view.selected = Some(target.min(n - 1));
            view.reveal = true;
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.view = None;
        }
    }

    /// The header and the panes, in the chosen layout.
    fn contents(&mut self, ui: &mut Ui, env: &mut Env) {
        let c = colors(ui);
        self.header(ui, &c, env);
        let body = ui.available_rect_before_wrap();
        self.body(ui, body, env);
    }

    /// The range at the top left, as TortoiseGit shows it; on the right the commit count, the
    /// layout picker and the button that resets the layout's dividers.
    fn header(&self, ui: &mut Ui, c: &Colors, env: &mut Env) {
        let Some(view) = &self.view else { return };
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
        let mut tools = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(vec2(8.0, 0.0)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        tools.spacing_mut().item_spacing.x = 4.0;
        layout_tools(&mut tools, env.settings);
        let tools_left = tools.min_rect().left();
        let painter = ui.painter();
        painter.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0, c.line),
        );
        let label = view
            .query
            .label(&view.repo, &view.refs, |r| env.graph.shows(r.kind));
        let font = FontId::proportional(13.5);
        let weak = ui.visuals().weak_text_color();
        let mut job = LayoutJob::default();
        let format = |color| TextFormat::simple(font.clone(), color);
        if let Some(from) = &label.from {
            job.append(from, 0.0, format(c.link));
            job.append("..", 2.0, format(weak));
            job.append(&label.to, 2.0, format(c.link));
        } else {
            job.append(&label.to, 0.0, format(c.link));
        }
        let n = view.commits.len();
        let count = format!("{} commit{}", thousands(n), if n == 1 { "" } else { "s" });
        let count = painter.layout_no_wrap(count, FontId::proportional(13.0), weak);
        let count_left = tools_left - 14.0 - count.size().x;
        job.wrap = TextWrapping {
            max_width: (count_left - rect.left() - 36.0).max(0.0),
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('…'),
        };
        let galley = painter.layout_job(job);
        painter.galley(
            pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0),
            galley,
            weak,
        );
        painter.galley(
            pos2(count_left, rect.center().y - count.size().y / 2.0),
            count,
            weak,
        );
    }

    /// The panes where the layout puts them, and the dividers between them.
    fn body(&mut self, ui: &mut Ui, body: Rect, env: &mut Env) {
        let layout = env.settings.layout;
        let arrangement = arrange(layout, env.settings.dividers.of(layout), body);
        let panes = [Pane::Commits, Pane::Details, Pane::Files];
        for (pane, rect) in panes.into_iter().zip(arrangement.panes) {
            self.pane(ui, pane, rect, env);
        }
        for (which, bar) in arrangement.bars.iter().enumerate() {
            let id = Id::new(("log-divider", layout, which));
            if let Some(at) = divider(ui, id, bar) {
                env.settings.dividers.set(layout, which, bar.fraction(at));
            }
        }
        ui.allocate_rect(body, Sense::hover());
    }

    /// Draws `pane` in `rect`.
    fn pane(&mut self, ui: &mut Ui, pane: Pane, rect: Rect, env: &mut Env) {
        let c = colors(ui);
        ui.painter().rect_filled(rect, 0.0, c.pane);
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .id_salt(("log-pane", pane as u8)),
        );
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        match pane {
            Pane::Commits => self.commits_pane(&mut child, env, &c),
            Pane::Details => self.details_pane(&mut child, env, &c),
            Pane::Files => self.files_pane(&mut child, env, &c),
        }
    }

    /// The commit list: short hash, ref badges and subject, author, date. Virtualised; the rows
    /// are painted directly.
    fn commits_pane(&mut self, ui: &mut Ui, env: &mut Env, c: &Colors) {
        let Some(view) = &mut self.view else { return };
        let mono = FontId::monospace(12.0);
        let body = egui::TextStyle::Body.resolve(ui.style());
        let digit = ui
            .painter()
            .layout_no_wrap("0".into(), mono.clone(), c.line)
            .size()
            .x;
        let hash_width = digit * view.repo.abbrev_len as f32 + 2.0 * CELL_PAD + 2.0;
        let weak = ui.visuals().weak_text_color();
        let text = ui.visuals().text_color();
        let (author_width, date_width) = if ui.available_width() < NARROW_LIST {
            (NARROW_AUTHOR_WIDTH, NARROW_DATE_WIDTH)
        } else {
            (AUTHOR_WIDTH, DATE_WIDTH)
        };

        let columns = |width: f32, left: f32| {
            let subject = (width - hash_width - author_width - date_width).max(80.0);
            let x = [
                left,
                left + hash_width,
                left + hash_width + subject,
                left + hash_width + subject + author_width,
            ];
            let w = [hash_width, subject, author_width, date_width];
            (x, w)
        };

        // Column headings.
        let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADING), Sense::hover());
        heading_background(ui, head, c);
        let (x, w) = columns(head.width(), head.left());
        for (i, title) in ["Hash", "Subject", "Author", "Date"]
            .into_iter()
            .enumerate()
        {
            let g = cell(
                ui,
                title,
                FontId::proportional(12.0),
                weak,
                w[i] - 2.0 * CELL_PAD,
            );
            ui.painter().galley(
                pos2(x[i] + CELL_PAD, head.center().y - g.size().y / 2.0),
                g,
                weak,
            );
        }

        if view.commits.is_empty() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| ui.weak("No commits."));
            return;
        }

        ui.spacing_mut().item_spacing.y = 0.0;
        let mut area = ScrollArea::vertical()
            .id_salt(("log-commits", view.id))
            .auto_shrink(false);
        if view.reveal
            && let Some(sel) = view.selected
        {
            view.reveal = false;
            let top = sel as f32 * ROW;
            let height = view.list_height.max(ROW);
            if top < view.scroll {
                area = area.vertical_scroll_offset(top);
            } else if top + ROW > view.scroll + height {
                area = area.vertical_scroll_offset(top + ROW - height);
            }
        }
        let mut clicked = None;
        let output = area.show_rows(ui, ROW, view.commits.len(), |ui, range| {
            for i in range {
                let commit = view.repo.commit(view.commits[i]);
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                let selected = view.selected == Some(i);
                let bg = if selected {
                    Some(c.selected_bg)
                } else if response.hovered() {
                    Some(c.hover)
                } else if i % 2 == 1 {
                    Some(c.stripe)
                } else {
                    None
                };
                if let Some(bg) = bg {
                    ui.painter().rect_filled(rect, 0.0, bg);
                }
                let (fg, fg_weak) = if selected {
                    (c.selected_fg, c.selected_fg)
                } else {
                    (text, weak)
                };
                let (x, w) = columns(rect.width(), rect.left());
                let y = rect.center().y;
                let painter = ui.painter();
                let put = |g: Arc<Galley>, x: f32, color| {
                    painter.galley(pos2(x, y - g.size().y / 2.0), g, color);
                };
                let hash = commit.oid.short(view.repo.abbrev_len);
                put(
                    cell(ui, &hash, mono.clone(), fg_weak, w[0]),
                    x[0] + CELL_PAD,
                    fg_weak,
                );

                // Ref badges, then the subject in what is left.
                let mut left = x[1] + CELL_PAD;
                let right = x[1] + w[1] - CELL_PAD;
                for &r in &view.refs[view.commits[i].ix()] {
                    let git_ref = &view.repo.refs[r];
                    if !env.graph.shows(git_ref.kind) || left >= right {
                        continue;
                    }
                    left += badge(ui, git_ref, &env.palette, pos2(left, y), right - left) + 4.0;
                }
                if left < right {
                    put(
                        cell(ui, &commit.subject, body.clone(), fg, right - left),
                        left,
                        fg,
                    );
                }
                put(
                    cell(
                        ui,
                        &commit.author_name,
                        body.clone(),
                        fg,
                        w[2] - 2.0 * CELL_PAD,
                    ),
                    x[2] + CELL_PAD,
                    fg,
                );
                put(
                    cell(
                        ui,
                        &commit.author_date,
                        body.clone(),
                        fg_weak,
                        w[3] - 2.0 * CELL_PAD,
                    ),
                    x[3] + CELL_PAD,
                    fg_weak,
                );
                let author = Rect::from_x_y_ranges(x[2]..=x[2] + w[2], rect.y_range());
                let over_author = response.hover_pos().is_some_and(|p| author.contains(p));
                let response = if over_author {
                    response
                        .on_hover_text(format!("{} <{}>", commit.author_name, commit.author_email))
                } else {
                    response
                };
                if response.clicked() {
                    clicked = Some(i);
                }
            }
        });
        view.scroll = output.state.offset.y;
        view.list_height = output.inner_rect.height();
        if let Some(i) = clicked {
            view.selected = Some(i);
        }
    }

    /// The selected commit: full hash, author and date, each with a Copy button where it helps,
    /// then the full message. The text is selectable, and web links open in the browser.
    fn details_pane(&mut self, ui: &mut Ui, env: &mut Env, c: &Colors) {
        let Some(view) = &self.view else { return };
        let Some(ix) = view.selected_commit() else {
            return;
        };
        let commit = view.repo.commit(ix);
        let now = ui.input(|i| i.time);
        let copied = self
            .copied
            .filter(|&(_, at)| now - at < COPIED_SECONDS)
            .map(|(what, _)| what);
        if copied.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
        let mut copy = None;
        let ctx = ui.ctx().clone();
        let message = env.messages.get(&view.repo.path, commit.oid, &ctx);
        ScrollArea::vertical()
            .id_salt(("log-details", commit.oid))
            .auto_shrink(false)
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::Grid::new("log-meta")
                            .num_columns(2)
                            .spacing(vec2(12.0, 4.0))
                            .show(ui, |ui| {
                                ui.weak("Commit");
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(commit.oid.to_hex()).monospace());
                                    if copy_button(ui, copied == Some(Copied::Hash), c)
                                        .on_hover_text("Copy the full hash")
                                        .clicked()
                                    {
                                        ui.ctx().copy_text(commit.oid.to_hex());
                                        copy = Some(Copied::Hash);
                                    }
                                });
                                ui.end_row();
                                ui.weak("Author");
                                ui.horizontal(|ui| {
                                    ui.label(format!(
                                        "{} <{}>",
                                        commit.author_name, commit.author_email
                                    ));
                                    if copy_button(ui, copied == Some(Copied::Email), c)
                                        .on_hover_text("Copy the email address")
                                        .clicked()
                                    {
                                        ui.ctx().copy_text(commit.author_email.clone());
                                        copy = Some(Copied::Email);
                                    }
                                });
                                ui.end_row();
                                ui.weak("Date");
                                ui.label(&commit.author_date);
                                ui.end_row();
                            });
                        ui.add_space(10.0);
                        match message {
                            Some(message) => message_ui(ui, message),
                            None => {
                                message_ui(ui, &commit.subject);
                                ui.spinner();
                            }
                        }
                    });
            });
        if let Some(what) = copy {
            self.copied = Some((what, now));
        }
    }

    /// The selected commit's changed files: a filter and a sortable table.
    fn files_pane(&mut self, ui: &mut Ui, _env: &mut Env, c: &Colors) {
        let Some(view) = &self.view else { return };
        let Some(ix) = view.selected_commit() else {
            return;
        };
        let commit = view.repo.commit(ix);
        let merge = commit.parents.len() > 1;
        let ctx = ui.ctx().clone();
        let files = self.files.get(&view.repo.path, commit.oid, &ctx);
        let shown = match files {
            Some(Ok(files)) => Some(filter_and_sort(files, &self.filter, self.order)),
            _ => None,
        };
        let weak = ui.visuals().weak_text_color();

        // The bar: filter, merge note, count.
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
        heading_background(ui, bar, c);
        let mut bar_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(bar.shrink2(vec2(8.0, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        filter_field(&mut bar_ui, &mut self.filter, 280.0);
        if merge {
            bar_ui.label(
                RichText::new("Merge: compared with its first parent")
                    .size(12.0)
                    .color(weak),
            );
        }
        if let (Some(Ok(files)), Some(shown)) = (files, &shown) {
            let count = if self.filter.is_empty() {
                let n = files.len();
                format!("{} file{}", thousands(n), if n == 1 { "" } else { "s" })
            } else {
                format!(
                    "{} of {} files",
                    thousands(shown.len()),
                    thousands(files.len())
                )
            };
            bar_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(count).size(12.0).color(weak));
            });
        }

        let files = match files {
            None => {
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.weak("Loading…");
                });
                return;
            }
            Some(Err(e)) => {
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.colored_label(c.removed, format!("Could not list the changed files: {e}"));
                });
                return;
            }
            Some(Ok(files)) => files,
        };
        let shown = shown.unwrap_or_default();

        // Headings; a click sorts, another reverses.
        let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADING), Sense::hover());
        heading_background(ui, head, c);
        let (x, w) = file_columns(head.width(), head.left());
        let compact = compact_files(head.width());
        let heading_font = FontId::proportional(12.0);
        for (i, column) in FileColumn::ALL.into_iter().enumerate() {
            let rect = Rect::from_x_y_ranges(x[i]..=x[i] + w[i], head.y_range());
            let mut response = ui.interact(rect, Id::new(("log-sort", i)), Sense::click());
            let heading = file_heading(column, compact);
            if heading != column.title() {
                response = response.on_hover_text(column.title());
            }
            if response.clicked() {
                self.order.click(column);
            }
            let color = if response.hovered() {
                ui.visuals().text_color()
            } else {
                weak
            };
            let g = cell(
                ui,
                heading,
                heading_font.clone(),
                color,
                w[i] - 2.0 * CELL_PAD - 12.0,
            );
            let numeric = matches!(column, FileColumn::Added | FileColumn::Removed);
            let arrow_room = if self.order.column == column {
                12.0
            } else {
                0.0
            };
            let left = if numeric {
                rect.right() - CELL_PAD - g.size().x - arrow_room
            } else {
                rect.left() + CELL_PAD
            };
            let text_width = g.size().x;
            ui.painter()
                .galley(pos2(left, rect.center().y - g.size().y / 2.0), g, color);
            if self.order.column == column {
                let at = pos2(left + text_width + 7.0, rect.center().y);
                sort_arrow(ui, at, self.order.descending, color);
            }
        }

        if shown.is_empty() {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.weak(if files.is_empty() {
                    "No changes."
                } else {
                    "No file matches the filter."
                });
            });
            return;
        }

        ui.spacing_mut().item_spacing.y = 0.0;
        let body = egui::TextStyle::Body.resolve(ui.style());
        let text = ui.visuals().text_color();
        ScrollArea::vertical()
            .id_salt(("log-files", commit.oid))
            .auto_shrink(false)
            .show_rows(ui, FILE_ROW, shown.len(), |ui, range| {
                for row in range {
                    let file = &files[shown[row]];
                    let (rect, response) = ui
                        .allocate_exact_size(vec2(ui.available_width(), FILE_ROW), Sense::hover());
                    if response.hovered() {
                        ui.painter().rect_filled(rect, 0.0, c.hover);
                    } else if row % 2 == 1 {
                        ui.painter().rect_filled(rect, 0.0, c.stripe);
                    }
                    let (x, w) = file_columns(rect.width(), rect.left());
                    let y = rect.center().y;
                    let put = |g: Arc<Galley>, x: f32| {
                        ui.painter().galley(pos2(x, y - g.size().y / 2.0), g, text);
                    };
                    let put_right = |g: Arc<Galley>, right: f32| {
                        ui.painter().galley(
                            pos2(right - g.size().x, y - g.size().y / 2.0),
                            g,
                            text,
                        );
                    };
                    let pieces = path_pieces(file);
                    let (path, elided) =
                        path_galley(ui, &pieces, &body, w[0] - 2.0 * CELL_PAD, text, weak);
                    put(path, x[0] + CELL_PAD);
                    put(
                        cell(
                            ui,
                            file.extension(),
                            body.clone(),
                            weak,
                            w[1] - 2.0 * CELL_PAD,
                        ),
                        x[1] + CELL_PAD,
                    );
                    let status_color = match file.status {
                        FileStatus::Added => c.added,
                        FileStatus::Deleted => c.removed,
                        FileStatus::Renamed | FileStatus::Copied => c.renamed,
                        _ => text,
                    };
                    put(
                        cell(
                            ui,
                            file.status.name(),
                            body.clone(),
                            status_color,
                            w[2] - 2.0 * CELL_PAD,
                        ),
                        x[2] + CELL_PAD,
                    );
                    // Binary files have no line counts.
                    let count = |n: Option<u32>, color| {
                        let (s, color) = match n {
                            Some(n) => (thousands(n as usize), color),
                            None => ("–".to_owned(), weak),
                        };
                        ui.painter().layout_no_wrap(s, body.clone(), color)
                    };
                    put_right(count(file.added, c.added), x[3] + w[3] - CELL_PAD);
                    put_right(count(file.removed, c.removed), x[4] + w[4] - CELL_PAD);
                    let path_rect = Rect::from_x_y_ranges(x[0]..=x[0] + w[0], rect.y_range());
                    if elided && response.hover_pos().is_some_and(|p| path_rect.contains(p)) {
                        response.on_hover_ui(|ui| {
                            ui.label(path_job(&pieces, 0, &body, text, weak));
                        });
                    }
                }
            });
    }
}

/// A piece of a path cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathPiece {
    Folder,
    Name,
    /// The ` → ` between the old and new path of a rename.
    Arrow,
}

/// A file's path in pieces: `old → new` for renames and copies.
fn path_pieces(file: &ChangedFile) -> Vec<(String, PathPiece)> {
    let mut pieces = Vec::new();
    let push_path = |pieces: &mut Vec<(String, PathPiece)>, path: &str| match path.rfind('/') {
        Some(i) => {
            pieces.push((path[..=i].to_owned(), PathPiece::Folder));
            pieces.push((path[i + 1..].to_owned(), PathPiece::Name));
        }
        None => pieces.push((path.to_owned(), PathPiece::Name)),
    };
    if let Some(old) = &file.old_path {
        push_path(&mut pieces, old);
        pieces.push((" → ".to_owned(), PathPiece::Arrow));
    }
    push_path(&mut pieces, &file.path);
    pieces
}

/// The pieces from byte `from` of their text on, after an ellipsis if `from` isn't 0: folders
/// weak, file names in the text colour. The arrow is monospaced: the proportional font has
/// none.
fn path_job(
    pieces: &[(String, PathPiece)],
    from: usize,
    font: &FontId,
    text: Color32,
    weak: Color32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    if from > 0 {
        job.append("…", 0.0, TextFormat::simple(font.clone(), weak));
    }
    let mut at = 0;
    for (piece, kind) in pieces {
        let (start, end) = (at, at + piece.len());
        at = end;
        if end <= from {
            continue;
        }
        let piece = &piece[start.max(from) - start..];
        let format = match kind {
            PathPiece::Folder => TextFormat::simple(font.clone(), weak),
            PathPiece::Name => TextFormat::simple(font.clone(), text),
            PathPiece::Arrow => TextFormat::simple(FontId::monospace(font.size), weak),
        };
        job.append(piece, 0.0, format);
    }
    job
}

/// The path cell, cut at the start when it is too long so that the file name stays in view.
/// Also says whether it was cut.
fn path_galley(
    ui: &Ui,
    pieces: &[(String, PathPiece)],
    font: &FontId,
    width: f32,
    text: Color32,
    weak: Color32,
) -> (Arc<Galley>, bool) {
    let full: String = pieces.iter().map(|(s, _)| s.as_str()).collect();
    let painter = ui.painter();
    // A candidate is `full`, or "…" and an end of `full`.
    let job = |shown: &str| {
        let from = if shown == full {
            0
        } else {
            full.len() + '…'.len_utf8() - shown.len()
        };
        path_job(pieces, from, font, text, weak)
    };
    let shown = elide_start(&full, |s| painter.layout_job(job(s)).size().x <= width);
    (painter.layout_job(job(&shown)), shown != full)
}

/// Whether a changed-files table `width` wide is compact: in a narrow pane (beside another
/// one in layouts B and D) the columns after the path shrink, with shorter headings, so that
/// the path keeps room.
fn compact_files(width: f32) -> bool {
    const ROOMY_PATH: f32 = 260.0;
    width - FILE_COLUMNS.iter().sum::<f32>() < ROOMY_PATH
}

/// Widths of the columns after the path: extension, status, lines added, lines removed.
const FILE_COLUMNS: [f32; 4] = [90.0, 100.0, 100.0, 110.0];
const COMPACT_FILE_COLUMNS: [f32; 4] = [56.0, 78.0, 66.0, 82.0];

/// A changed-files column's heading; shorter in a compact table.
fn file_heading(column: FileColumn, compact: bool) -> &'static str {
    match column {
        FileColumn::Extension if compact => "Ext.",
        FileColumn::Added if compact => "Added",
        FileColumn::Removed if compact => "Removed",
        _ => column.title(),
    }
}

/// x and widths of the changed-files columns: the path takes what the others leave.
fn file_columns(width: f32, left: f32) -> ([f32; 5], [f32; 5]) {
    let fixed = if compact_files(width) {
        COMPACT_FILE_COLUMNS
    } else {
        FILE_COLUMNS
    };
    let path = (width - fixed.iter().sum::<f32>()).max(120.0);
    let w = [path, fixed[0], fixed[1], fixed[2], fixed[3]];
    let mut x = [left; 5];
    for i in 1..5 {
        x[i] = x[i - 1] + w[i - 1];
    }
    (x, w)
}

/// `text` on one line, cut with an ellipsis at `width`.
fn cell(ui: &Ui, text: &str, font: FontId, color: Color32, width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = TextWrapping {
        max_width: width.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

/// A ref's badge in the graph's label colour, left-centred at `at` and at most `max_width`
/// wide. Returns its width.
fn badge(ui: &Ui, git_ref: &GitRef, palette: &Palette, at: egui::Pos2, max_width: f32) -> f32 {
    let fill = palette.ref_fill(git_ref.kind, git_ref.is_head, &git_ref.name);
    let color = text_on(fill);
    let pad = 5.0;
    let g = cell(
        ui,
        &git_ref.name,
        FontId::proportional(11.5),
        color,
        max_width - 2.0 * pad,
    );
    let size = vec2(g.size().x + 2.0 * pad, 17.0);
    let rect = Rect::from_min_size(pos2(at.x, at.y - size.y / 2.0), size);
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(3),
        fill,
        Stroke::new(1.0, Color32::from_black_alpha(64)),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        pos2(rect.left() + pad, rect.center().y - g.size().y / 2.0),
        g,
        color,
    );
    size.x
}

/// The background of column headings and bars: the panel colour with a line below.
fn heading_background(ui: &Ui, rect: Rect, c: &Colors) {
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, ui.visuals().panel_fill);
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, c.line),
    );
}

/// A small triangle: up for ascending, down for descending.
fn sort_arrow(ui: &Ui, at: egui::Pos2, descending: bool, color: Color32) {
    let (w, h) = (3.5, 3.0);
    let points = if descending {
        vec![
            at + vec2(-w, -h / 2.0),
            at + vec2(w, -h / 2.0),
            at + vec2(0.0, h),
        ]
    } else {
        vec![
            at + vec2(-w, h / 2.0),
            at + vec2(w, h / 2.0),
            at + vec2(0.0, -h),
        ]
    };
    ui.painter()
        .add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

/// A divider between two panes. While it is dragged, returns where its middle should go: the
/// pointer's position across the bar, less where on the bar it was grabbed.
fn divider(ui: &mut Ui, id: Id, bar: &Bar) -> Option<f32> {
    let c = colors(ui);
    let rect = bar.rect;
    let cursor = if bar.vertical {
        CursorIcon::ResizeHorizontal
    } else {
        CursorIcon::ResizeVertical
    };
    let response = ui.interact(rect, id, Sense::drag()).on_hover_cursor(cursor);
    let active = response.hovered() || response.dragged();
    let painter = ui.painter();
    let fill = if active {
        widgets::tones(ui).accent
    } else {
        ui.visuals().panel_fill
    };
    painter.rect_filled(rect, 0.0, fill);
    let line = Stroke::new(1.0, c.line);
    if bar.vertical {
        painter.vline(rect.left() + 0.5, rect.y_range(), line);
        painter.vline(rect.right() - 0.5, rect.y_range(), line);
    } else {
        painter.hline(rect.x_range(), rect.top() + 0.5, line);
        painter.hline(rect.x_range(), rect.bottom() - 0.5, line);
    }
    let along = |p: egui::Pos2| if bar.vertical { p.x } else { p.y };
    let pointer = response.interact_pointer_pos().map(along)?;
    if response.drag_started() {
        let middle = along(rect.center());
        ui.data_mut(|d| d.insert_temp(id, pointer - middle));
    }
    if !response.dragged() {
        return None;
    }
    let grabbed = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(0.0);
    Some(pointer - grabbed)
}

/// The layout picker and the reset button, laid out right to left.
fn layout_tools(ui: &mut Ui, settings: &mut LogWindowSettings) {
    let current = settings.layout;
    let reset = ui.add_enabled_ui(!settings.dividers.is_default(current), |ui| {
        widgets::icon_button(ui, glyphs::RESET, false)
    });
    let reset = widgets::tip_explained(
        reset.inner,
        "Reset layout",
        "",
        "Put the dividers of this layout back where they started.",
    );
    if reset.clicked() {
        settings.dividers.reset(current);
    }
    if let Some(layout) = layout_picker(ui, current) {
        settings.layout = layout;
    }
}

/// The four layouts as icon segments, each named in its tooltip. Returns the one clicked.
pub fn layout_picker(ui: &mut Ui, current: LogLayout) -> Option<LogLayout> {
    let items = LogLayout::ALL.map(|l| (l, layout_glyph(l)));
    widgets::segmented(ui, current, &items, |layout, response| {
        response.on_hover_text(layout.label())
    })
}

/// The filter field of the changed files, with a magnifier.
fn filter_field(ui: &mut Ui, text: &mut String, width: f32) -> Response {
    let id = Id::new("log-filter");
    let focused = ui.memory(|m| m.has_focus(id));
    let t = widgets::tones(ui);
    let stroke = if focused {
        Stroke::new(1.5, t.accent)
    } else {
        Stroke::new(1.0, t.field_line)
    };
    egui::Frame::new()
        .fill(t.field)
        .stroke(stroke)
        .corner_radius(7)
        .inner_margin(Margin {
            left: 8,
            right: 6,
            top: 0,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.set_width(width - 14.0);
            ui.set_height(28.0);
            ui.spacing_mut().item_spacing.x = 6.0;
            let weak = ui.visuals().weak_text_color();
            let (icon, _) = ui.allocate_exact_size(Vec2::splat(15.0), Sense::hover());
            widgets::paint_glyph(ui.painter(), icon, parterre_core::glyphs::SEARCH, weak);
            ui.add(
                egui::TextEdit::singleline(text)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .hint_text("Filter paths")
                    .desired_width(ui.available_width()),
            )
        })
        .inner
}

/// A small, flat "Copy" button, which says "Copied" for a moment after a click.
fn copy_button(ui: &mut Ui, copied: bool, c: &Colors) -> Response {
    let (text, color) = if copied {
        ("Copied", c.added)
    } else {
        ("Copy", ui.visuals().weak_text_color())
    };
    let g = ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(12.0), color);
    let (rect, response) = ui.allocate_exact_size(vec2(g.size().x + 12.0, 20.0), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), c.hover);
    }
    ui.painter().galley(
        pos2(rect.left() + 6.0, rect.center().y - g.size().y / 2.0),
        g,
        color,
    );
    response
}

/// A commit message, subject in the strong colour, monospaced as in a terminal. The text is
/// selectable; `http(s)` links are clickable.
fn message_ui(ui: &mut Ui, message: &str) {
    let font = FontId::monospace(12.5);
    let text = ui.visuals().text_color();
    let strong = ui.visuals().strong_text_color();
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let label = |ui: &mut Ui, s: &str, color: Color32| {
        ui.add(egui::Label::new(RichText::new(s).font(font.clone()).color(color)).selectable(true));
    };
    // Lines without links are shown together, as one label.
    let mut plain = String::new();
    let flush = |ui: &mut Ui, plain: &mut String| {
        if !plain.is_empty() {
            label(ui, plain.trim_end_matches('\n'), text);
            plain.clear();
        }
    };
    for (n, line) in message.trim_end().lines().enumerate() {
        let urls = find_urls(line);
        let color = if n == 0 { strong } else { text };
        if urls.is_empty() && n > 0 {
            plain.push_str(if line.is_empty() { " " } else { line });
            plain.push('\n');
            continue;
        }
        flush(ui, &mut plain);
        if urls.is_empty() {
            label(ui, line, color);
            continue;
        }
        ui.horizontal_wrapped(|ui| {
            let mut at = 0;
            for url in urls {
                if url.start > at {
                    label(ui, &line[at..url.start], color);
                }
                let link = RichText::new(&line[url.clone()])
                    .font(font.clone())
                    .color(widgets::tones(ui).on_fg);
                ui.hyperlink_to(link, &line[url.clone()]);
                at = url.end;
            }
            if at < line.len() {
                label(ui, &line[at..], color);
            }
        });
    }
    flush(ui, &mut plain);
}

impl ParterreApp {
    /// Show log on graph nodes: one gives the node's log, two the range between them in the
    /// order they were selected; anything else does nothing.
    pub(super) fn show_log(&mut self, nodes: &[usize]) {
        let Some(scene) = &self.scene else { return };
        // The newest snapshot, which the scene on screen may not have caught up with yet.
        let Some(repo) = self.repo.clone() else {
            return;
        };
        let commits: Vec<CommitIx> = nodes
            .iter()
            .filter_map(|&n| {
                let oid = scene.repo.commit(scene.graph.nodes[n].commit).oid;
                repo.lookup(&oid)
            })
            .collect();
        if commits.len() != nodes.len() {
            return;
        }
        self.open_log(repo, &commits);
    }

    /// Opens the log of `commits` (one or two, see [`LogQuery::for_selection`]).
    pub(super) fn open_log(&mut self, repo: Arc<Repo>, commits: &[CommitIx]) {
        let Some(query) = LogQuery::for_selection(&repo, commits) else {
            return;
        };
        let was_open = self.log.is_open();
        let [w, h] = self.settings.log_window.size;
        self.log.open(repo, query, vec2(w, h));
        if was_open {
            self.focus_log = true;
        }
    }

    /// The log window, while it is open. (Screenshot runs embed it in the main window.)
    pub(super) fn log_window(&mut self, ctx: &egui::Context) {
        if !self.log.is_open() {
            self.log.title_theme = None;
            return;
        }
        let builder = egui::ViewportBuilder::default()
            .with_title(self.log.title())
            .with_app_id(crate::settings::APP_ID)
            .with_icon(self.window_icon.clone())
            .with_inner_size(self.log.size)
            .with_min_inner_size([480.0, 360.0]);
        let id = viewport_id();
        if self.log.title_theme.is_none() && !ctx.embed_viewports() {
            ctx.request_repaint();
        }
        if std::mem::take(&mut self.focus_log) && !ctx.embed_viewports() {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
        }
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            let embedded = class == egui::ViewportClass::EmbeddedWindow;
            if !embedded {
                if self.log.title_theme != self.window_theme {
                    self.log.title_theme = self.window_theme;
                    if let Some(theme) = self.window_theme {
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::SetTheme(theme));
                    }
                }
                let (close, size, reload) = ui.input(|i| {
                    (
                        i.viewport().close_requested(),
                        i.viewport().inner_rect.map(|r| r.size()),
                        i.key_pressed(Key::F5),
                    )
                });
                if let Some(size) = size
                    && size.x > 0.0
                    && size.y > 0.0
                {
                    self.settings.log_window.size = [size.x, size.y];
                }
                if reload {
                    self.reload();
                }
                // Keys go to the main window too when the log is embedded in it.
                self.log.handle_keys(ui);
                if close {
                    self.log.view = None;
                }
            }
            if !self.log.is_open() {
                return;
            }
            let palette = Palette::new(ui.visuals().dark_mode, &self.settings.branch_colors);
            let mut env = Env {
                messages: &mut self.messages,
                palette,
                graph: &self.settings.graph,
                settings: &mut self.settings.log_window,
            };
            let log = &mut self.log;
            egui::CentralPanel::default()
                .frame(egui::Frame::central_panel(&ui.ctx().global_style()).inner_margin(0))
                .show(ui, |ui| log.contents(ui, &mut env));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parterre_core::log_layout::Dividers;

    fn renamed() -> ChangedFile {
        ChangedFile {
            path: "src/new.rs".into(),
            old_path: Some("lib/old.rs".into()),
            status: FileStatus::Renamed,
            added: Some(1),
            removed: Some(0),
        }
    }

    #[test]
    fn renames_show_old_and_new_path_with_folders_apart() {
        let pieces = path_pieces(&renamed());
        let kinds: Vec<_> = pieces.iter().map(|(s, k)| (s.as_str(), *k)).collect();
        assert_eq!(
            kinds,
            [
                ("lib/", PathPiece::Folder),
                ("old.rs", PathPiece::Name),
                (" → ", PathPiece::Arrow),
                ("src/", PathPiece::Folder),
                ("new.rs", PathPiece::Name),
            ]
        );
        let job = path_job(
            &pieces,
            0,
            &FontId::default(),
            Color32::WHITE,
            Color32::GRAY,
        );
        assert_eq!(job.text, "lib/old.rs → src/new.rs");
    }

    #[test]
    fn a_cut_path_keeps_its_end_after_an_ellipsis() {
        let pieces = path_pieces(&renamed());
        let (text, weak) = (Color32::WHITE, Color32::GRAY);
        // Cut inside the first file name, then inside the second folder.
        let job = path_job(&pieces, 6, &FontId::default(), text, weak);
        assert_eq!(job.text, "…d.rs → src/new.rs");
        assert_eq!(job.sections[1].format.color, text);
        let from = "lib/old.rs → s".len();
        let job = path_job(&pieces, from, &FontId::default(), text, weak);
        assert_eq!(job.text, "…rc/new.rs");
        // The ellipsis and the folder share one weak section.
        let colors: Vec<_> = job.sections.iter().map(|s| s.format.color).collect();
        assert_eq!(colors, [weak, text]);
    }

    #[test]
    fn layouts_tile_the_body_with_their_panes_and_dividers() {
        let body = Rect::from_min_size(pos2(10.0, 50.0), vec2(1100.0, 700.0));
        let d = Dividers::default();
        for layout in LogLayout::ALL {
            let a = arrange(layout, d.of(layout), body);
            let rects: Vec<Rect> = a
                .panes
                .iter()
                .copied()
                .chain(a.bars.map(|b| b.rect))
                .collect();
            let area: f32 = rects.iter().map(|r| r.area()).sum();
            assert!(
                (area - body.area()).abs() < 1.0,
                "{layout:?} leaves gaps or overlaps"
            );
            for (i, r) in rects.iter().enumerate() {
                assert!(
                    body.expand(0.01).contains_rect(*r),
                    "{layout:?}: {r:?} outside"
                );
                for s in &rects[i + 1..] {
                    assert!(
                        r.intersect(*s).area() < 0.01,
                        "{layout:?}: {r:?} overlaps {s:?}"
                    );
                }
            }
            for (which, bar) in a.bars.iter().enumerate() {
                assert_eq!(bar.vertical, Dividers::splits_width(layout, which));
                let thickness = if bar.vertical {
                    bar.rect.width()
                } else {
                    bar.rect.height()
                };
                assert_eq!(thickness, DIVIDER);
            }
        }
    }

    #[test]
    fn a_divider_dragged_by_its_middle_stays_under_the_pointer() {
        let body = Rect::from_min_size(pos2(0.0, 40.0), vec2(1000.0, 600.0));
        for layout in LogLayout::ALL {
            for which in 0..2 {
                let mut d = Dividers::default();
                let before = arrange(layout, d.of(layout), body).bars[which];
                let along = |r: Rect| {
                    if before.vertical {
                        r.center().x
                    } else {
                        r.center().y
                    }
                };
                let target = along(before.rect) + 37.0;
                d.set(layout, which, before.fraction(target));
                let after = arrange(layout, d.of(layout), body).bars[which];
                assert!(
                    (along(after.rect) - target).abs() <= 0.5,
                    "{layout:?} divider {which}: {} instead of {target}",
                    along(after.rect)
                );
            }
        }
    }
}
