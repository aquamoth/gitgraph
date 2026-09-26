//! The changed-files table of the log and compare windows: a filter, sortable columns, a
//! selection, and double-click or Enter to open diff windows. Also the worker that asks git
//! for such lists off the UI thread.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::{Arc, mpsc};

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
    self, Color32, FontId, Galley, Id, Key, Margin, Modifiers, Rect, Response, RichText,
    ScrollArea, Sense, Stroke, Ui, UiBuilder, Vec2, pos2, vec2,
};
use parterre_core::changed_files::{
    ChangedFile, FileColumn, FileOrder, FileStatus, filter_and_sort,
};
use parterre_core::git::Git;
use parterre_core::text::{elide_start, thousands};

use super::log_window::{CELL_PAD, Colors, HEADING, cell, heading_background};
use crate::widgets;

/// Opening more diff windows than this at once asks first, as TortoiseGit does.
const MANY_DIFFS: usize = 10;
/// Height of a changed-file row.
const FILE_ROW: f32 = 23.0;

/// Changed files, or why they could not be listed.
pub type Listing = Result<Vec<ChangedFile>, String>;

/// The table's state: sort, filter and selection.
#[derive(Debug, Default)]
pub struct FileTable {
    pub order: FileOrder,
    pub filter: String,
    selection: FileSelection,
}

/// Changed files chosen in the list, by path, for the list they belong to. Showing another
/// list clears it.
#[derive(Debug, Default)]
struct FileSelection {
    owner: Option<Id>,
    paths: HashSet<String>,
    /// Where a Shift+click range starts.
    anchor: Option<String>,
}

/// What a click in the changed files asked for, by row in the list as shown.
enum FileClick {
    Select(usize, Modifiers),
    Open(usize),
}

impl FileTable {
    /// Forgets the selection.
    pub fn clear_selection(&mut self) {
        self.selection = FileSelection::default();
    }

    /// The bar (the filter, then what `bar` adds, and the count on the right), the headings
    /// and the rows of `files`, or a line saying they are loading or failed. `name` tells the
    /// window's widgets apart from another window's; `owner` tells lists apart: the selection
    /// and the scroll position belong to it. Returns the files to open (a double-click, or
    /// Enter on the selection).
    pub fn show<'f>(
        &mut self,
        ui: &mut Ui,
        c: &Colors,
        name: &str,
        owner: Id,
        files: Option<&'f Listing>,
        bar: impl FnOnce(&mut Ui),
    ) -> Vec<&'f ChangedFile> {
        if self.selection.owner != Some(owner) {
            self.selection = FileSelection {
                owner: Some(owner),
                ..FileSelection::default()
            };
        }
        let shown = match files {
            Some(Ok(files)) => Some(filter_and_sort(files, &self.filter, self.order)),
            _ => None,
        };
        let weak = ui.visuals().weak_text_color();

        // The bar: filter, what the window adds, count.
        let (bar_rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
        heading_background(ui, bar_rect, c);
        let mut bar_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(bar_rect.shrink2(vec2(8.0, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        filter_field(
            &mut bar_ui,
            &mut self.filter,
            Id::new((name, "filter")),
            280.0,
        );
        bar(&mut bar_ui);
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
                return Vec::new();
            }
            Some(Err(e)) => {
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.colored_label(c.removed, format!("Could not list the changed files: {e}"));
                });
                return Vec::new();
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
            let mut response = ui.interact(rect, Id::new((name, "sort", i)), Sense::click());
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
            return Vec::new();
        }

        ui.spacing_mut().item_spacing.y = 0.0;
        let body = egui::TextStyle::Body.resolve(ui.style());
        let text = ui.visuals().text_color();
        let mut click = None;
        let selection = &self.selection;
        ScrollArea::vertical()
            .id_salt(("files", owner))
            .auto_shrink(false)
            .show_rows(ui, FILE_ROW, shown.len(), |ui, range| {
                for row in range {
                    let file = &files[shown[row]];
                    let (rect, response) = ui
                        .allocate_exact_size(vec2(ui.available_width(), FILE_ROW), Sense::click());
                    if response.double_clicked() {
                        click = Some(FileClick::Open(row));
                    } else if response.clicked() {
                        click = Some(FileClick::Select(row, ui.input(|i| i.modifiers)));
                    }
                    if selection.paths.contains(&file.path) {
                        ui.painter().rect_filled(rect, 0.0, c.selected_bg);
                    }
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

        // Enter opens every selected file; a double-click opens one.
        let enter = !ui.ctx().egui_wants_keyboard_input()
            && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        let sel = &mut self.selection;
        let mut open = Vec::new();
        match click {
            Some(FileClick::Open(row)) => {
                let file = &files[shown[row]];
                sel.paths = HashSet::from([file.path.clone()]);
                sel.anchor = Some(file.path.clone());
                open.push(file);
            }
            Some(FileClick::Select(row, mods)) => {
                let path = files[shown[row]].path.clone();
                let anchor = sel
                    .anchor
                    .as_ref()
                    .and_then(|a| shown.iter().position(|&i| &files[i].path == a));
                if mods.command {
                    if !sel.paths.remove(&path) {
                        sel.paths.insert(path.clone());
                    }
                    sel.anchor = Some(path);
                } else if mods.shift
                    && let Some(from) = anchor
                {
                    let (a, b) = (from.min(row), from.max(row));
                    sel.paths = shown[a..=b]
                        .iter()
                        .map(|&i| files[i].path.clone())
                        .collect();
                } else {
                    sel.paths = HashSet::from([path.clone()]);
                    sel.anchor = Some(path);
                }
            }
            None => {}
        }
        if enter {
            open.extend(
                shown
                    .iter()
                    .map(|&i| &files[i])
                    .filter(|f| sel.paths.contains(&f.path)),
            );
        }
        open
    }
}

/// Diff windows asked for, held back while "Open all?" is asked when there are many.
#[derive(Debug)]
pub struct DiffQueue<T> {
    ready: Vec<T>,
    confirm: Option<Vec<T>>,
}

impl<T> Default for DiffQueue<T> {
    fn default() -> Self {
        DiffQueue {
            ready: Vec::new(),
            confirm: None,
        }
    }
}

impl<T> DiffQueue<T> {
    /// Queues `open`, or asks first when it is more than [`MANY_DIFFS`].
    pub fn push(&mut self, open: Vec<T>) {
        if open.len() > MANY_DIFFS {
            self.confirm = Some(open);
        } else {
            self.ready.extend(open);
        }
    }

    /// The diff windows to open now.
    pub fn take(&mut self) -> Vec<T> {
        std::mem::take(&mut self.ready)
    }

    /// Drops what is waiting for an answer.
    pub fn cancel(&mut self) {
        self.confirm = None;
    }

    /// Asks before opening more than [`MANY_DIFFS`] diff windows at once.
    pub fn confirm_many(&mut self, ui: &mut Ui, id: Id) {
        let Some(pending) = &self.confirm else { return };
        let n = pending.len();
        let (mut open, mut cancel) = (false, false);
        let modal = egui::Modal::new(id).show(ui.ctx(), |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(format!("Open {n} diff windows?")).strong());
            ui.add_space(4.0);
            ui.label("A window opens for every selected file.");
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                open = widgets::text_button(ui, "Open all").clicked();
                cancel = widgets::text_button(ui, "Cancel").clicked();
            });
        });
        if open {
            self.ready.extend(self.confirm.take().unwrap_or_default());
        } else if cancel || modal.should_close() {
            self.confirm = None;
        }
    }
}

/// Lists asked of git on a worker thread when first needed, and kept by key: the changed
/// files of a commit, or the files two commits differ in.
#[derive(Debug)]
pub struct Lister<K, V> {
    /// `None` while git works on it.
    cache: HashMap<K, Option<V>>,
    /// Results; `None` for a request dropped because a newer one came in.
    rx: Option<mpsc::Receiver<(K, Option<V>)>>,
    tx: Option<mpsc::Sender<K>>,
}

impl<K, V> Default for Lister<K, V> {
    fn default() -> Self {
        Lister {
            cache: HashMap::new(),
            rx: None,
            tx: None,
        }
    }
}

impl<K, V> Lister<K, V>
where
    K: Clone + Eq + Hash + Send + 'static,
    V: Send + 'static,
{
    /// The list for `key` if it is known; otherwise asks for it. The first call starts the
    /// worker, which runs `work` with git in `repo_path` for every key asked for.
    pub fn get(
        &mut self,
        repo_path: &std::path::Path,
        key: K,
        ctx: &egui::Context,
        work: fn(&Git, &K) -> V,
    ) -> Option<&V> {
        while let Some(Ok((key, value))) = self.rx.as_ref().map(|rx| rx.try_recv()) {
            match value {
                Some(value) => self.cache.insert(key, Some(value)),
                None => self.cache.remove(&key),
            };
        }
        if let std::collections::hash_map::Entry::Vacant(slot) = self.cache.entry(key.clone()) {
            slot.insert(None);
            let tx = self.tx.get_or_insert_with(|| {
                let (req_tx, req_rx) = mpsc::channel::<K>();
                let (res_tx, res_rx) = mpsc::channel();
                let git = Git::new(repo_path);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    while let Ok(mut key) = req_rx.recv() {
                        // Holding an arrow key asks for one commit after another; only the
                        // newest request still matters. The others are forgotten, so they are
                        // asked for again if they show up once more.
                        while let Ok(newer) = req_rx.try_recv() {
                            if res_tx.send((key, None)).is_err() {
                                return;
                            }
                            key = newer;
                        }
                        let value = work(&git, &key);
                        if res_tx.send((key, Some(value))).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                    }
                });
                self.rx = Some(res_rx);
                req_tx
            });
            let _ = tx.send(key.clone());
        }
        self.cache.get(&key).and_then(Option::as_ref)
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

/// The filter field of the changed files, with a magnifier.
fn filter_field(ui: &mut Ui, text: &mut String, id: Id, width: f32) -> Response {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn renamed() -> ChangedFile {
        ChangedFile {
            path: "src/new.rs".into(),
            old_path: Some("lib/old.rs".into()),
            status: FileStatus::Renamed,
            modes: [0o100644; 2],
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
}
