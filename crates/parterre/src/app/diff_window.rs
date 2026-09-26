//! Diff windows: one file diff each, in a window of its own (an immediate viewport, like the log
//! window). Several can be open at once; they outlive the log window they were opened from and
//! close with the repository. Decided in #45 and #46; the model is
//! [`parterre_core::file_diff`].
//!
//! A toolbar switches between side by side and unified, moves between changes, folds unchanged
//! stretches, and picks how changed words are found and what whitespace counts. The last
//! choices are kept in the settings for the next window. On the right an overview strip shows
//! where the changes are; long lines scroll sideways, both panes together.
//!
//! Deliberate deviations from TortoiseGitMerge (see `TODO.md`): unchanged stretches fold by
//! default; line endings count unless whitespace changes are ignored, and a note says when
//! they differ; changed words pair similar lines rather than lines by position; the change
//! marks are an overview strip on the right instead of a locator bar on the left.

use std::sync::{Arc, mpsc};

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
    self, Color32, CornerRadius, FontId, Key, Modifiers, Rect, RichText, ScrollArea, Sense, Stroke,
    StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2,
};
use parterre_core::changed_files::FileStatus;
use parterre_core::file_diff::{
    Content, DiffLine, DiffOptions, FileDiff, FileDiffSpec, LineKind, LoadedDiff, Note, Row, Shown,
    Version, Whitespace, WordMode, fold, fold_lines,
};
use parterre_core::glyphs;
use parterre_core::{Oid, Repo};

use super::ParterreApp;
use crate::settings::{DiffForm, DiffWindowSettings};
use crate::widgets;

/// Height of the toolbar.
const TOOLBAR: f32 = 44.0;
/// Height of the pane titles side by side.
const PANE_TITLE: f32 = 24.0;
const FONT_SIZE: f32 = 13.0;
/// Room for the `−`/`+` marker between the line numbers and the text.
const MARKER: f32 = 16.0;
/// Width of the overview strip.
const OVERVIEW: f32 = 14.0;
/// Height of the horizontal scrollbar.
const SCROLLBAR: f32 = 10.0;
/// Rows kept above a change scrolled to.
const LEAD: f32 = 3.0;

/// The open diff windows.
#[derive(Debug, Default)]
pub struct DiffWindows {
    windows: Vec<DiffWindow>,
    /// How many were opened, to give each its own viewport id.
    opened: u64,
}

impl DiffWindows {
    /// Opens a diff window for `spec`, or brings the one already showing it to the front.
    pub fn open(
        &mut self,
        repo: Arc<Repo>,
        spec: FileDiffSpec,
        settings: &DiffWindowSettings,
        ctx: &egui::Context,
    ) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.spec == spec) {
            w.focus = true;
            return;
        }
        self.opened += 1;
        self.windows
            .push(DiffWindow::new(self.opened, repo, spec, settings, ctx));
    }

    /// Closes every diff window (the repository they belong to is closing).
    pub fn close_all(&mut self) {
        self.windows.clear();
    }

    /// True while git or the diff is still working on any window.
    pub fn is_loading(&self) -> bool {
        self.windows
            .iter()
            .any(|w| matches!(w.load, Load::Loading(_)))
    }
}

/// A loaded diff and its model for the current options.
#[derive(Debug)]
struct Ready {
    loaded: LoadedDiff,
    diff: FileDiff,
    notes: Vec<Note>,
    options: DiffOptions,
}

impl Ready {
    fn new(loaded: LoadedDiff, options: DiffOptions) -> Ready {
        let diff = match &loaded.content {
            Content::Text { old, new, .. } => FileDiff::new(old, new, options),
            _ => FileDiff::default(),
        };
        let notes = diff.notes(&loaded, options.whitespace);
        Ready {
            loaded,
            diff,
            notes,
            options,
        }
    }
}

#[derive(Debug)]
enum Load {
    /// git and the diff run on a worker thread.
    Loading(mpsc::Receiver<Result<Ready, String>>),
    Ready(Box<Ready>),
    Failed(String),
}

/// Which version a line number belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Old,
    New,
}

/// Lines chosen on their line numbers, for copying: a run on one side, by line index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Selection {
    side: Side,
    anchor: u32,
    end: u32,
}

impl Selection {
    fn contains(&self, side: Side, line: u32) -> bool {
        side == self.side && (self.anchor.min(self.end)..=self.anchor.max(self.end)).contains(&line)
    }
}

#[derive(Debug)]
struct DiffWindow {
    id: u64,
    repo: Arc<Repo>,
    spec: FileDiffSpec,
    /// The size the window opened with (the viewport builder must not change while it is open).
    size: Vec2,
    load: Load,
    form: DiffForm,
    options: DiffOptions,
    fold: bool,
    /// Lines of the new version whose folds were opened by a click. Kept by line, so they stay
    /// open when the form, word mode or whitespace setting changes; the fold button folds them
    /// again.
    open: Vec<std::ops::Range<u32>>,
    /// The rows shown in the current form, folds included; rebuilt when `dirty`.
    shown: Vec<Shown>,
    /// Where each change's first row is in `shown`.
    positions: Vec<usize>,
    dirty: bool,
    /// The change in view (index into the form's changes), if the view is at or past one.
    current: Option<usize>,
    /// Scroll to this change in the next frame.
    jump: Option<usize>,
    /// Scroll to this offset in the next frame (a click in the overview).
    scroll_to: Option<f32>,
    /// The offset a jump to `current` left the view at: while the view stays there, that
    /// change stays current even if the view couldn't scroll to it (a short file).
    pinned: Option<f32>,
    /// Sideways scroll of the text, in points, shared by both panes.
    hoff: f32,
    selection: Option<Selection>,
    /// Bring the window to the front in the next frame.
    focus: bool,
    closed: bool,
    /// The theme last given to the window's title bar.
    title_theme: Option<egui::SystemTheme>,
}

impl DiffWindow {
    fn new(
        id: u64,
        repo: Arc<Repo>,
        spec: FileDiffSpec,
        settings: &DiffWindowSettings,
        ctx: &egui::Context,
    ) -> DiffWindow {
        let options = DiffOptions {
            words: settings.words,
            whitespace: settings.whitespace,
        };
        let (tx, rx) = mpsc::channel();
        let git = parterre_core::git::Git::new(&repo.path);
        let job = spec.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = git
                .load_file_diff(&job)
                .map(|loaded| Ready::new(loaded, options))
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        DiffWindow::new_loading(id, repo, spec, settings, rx)
    }

    /// A window waiting for `rx` to bring its diff.
    fn new_loading(
        id: u64,
        repo: Arc<Repo>,
        spec: FileDiffSpec,
        settings: &DiffWindowSettings,
        rx: mpsc::Receiver<Result<Ready, String>>,
    ) -> DiffWindow {
        let [w, h] = settings.size;
        DiffWindow {
            id,
            repo,
            spec,
            size: vec2(w, h),
            load: Load::Loading(rx),
            form: settings.form,
            options: DiffOptions {
                words: settings.words,
                whitespace: settings.whitespace,
            },
            fold: settings.fold,
            open: Vec::new(),
            shown: Vec::new(),
            positions: Vec::new(),
            dirty: true,
            current: None,
            jump: Some(0),
            scroll_to: None,
            pinned: None,
            hoff: 0.0,
            selection: None,
            focus: false,
            closed: false,
            title_theme: None,
        }
    }

    fn viewport_id(&self) -> egui::ViewportId {
        egui::ViewportId::from_hash_of(("diff", self.id))
    }

    /// `<file> (<short hash>) – <repo> – Diff`
    fn title(&self) -> String {
        let path = self.spec.path();
        let file = path.rsplit('/').next().unwrap_or(path);
        let rev = self
            .spec
            .new
            .as_ref()
            .or(self.spec.old.as_ref())
            .map(|v| v.rev.short(self.repo.abbrev_len))
            .unwrap_or_default();
        format!("{file} ({rev}) – {} – Diff", self.repo.display_name())
    }

    /// Takes the worker's result when it is there.
    fn poll(&mut self) {
        if let Load::Loading(rx) = &self.load
            && let Ok(result) = rx.try_recv()
        {
            self.load = match result {
                Ok(ready) => Load::Ready(Box::new(ready)),
                Err(e) => Load::Failed(e),
            };
            self.dirty = true;
        }
        // Options changed while the worker ran, or since.
        if let Load::Ready(ready) = &mut self.load
            && ready.options != self.options
        {
            let loaded = ready.loaded.clone();
            **ready = Ready::new(loaded, self.options);
            self.dirty = true;
            self.jump = Some(self.current.unwrap_or(0));
        }
    }

    fn ready(&self) -> Option<&Ready> {
        match &self.load {
            Load::Ready(r) => Some(r),
            _ => None,
        }
    }

    fn form_ix(&self) -> usize {
        match self.form {
            DiffForm::SideBySide => 0,
            DiffForm::Unified => 1,
        }
    }

    /// The rows of the current form, and where its changes start.
    fn rows_and_changes(diff: &FileDiff, form: DiffForm) -> (&[Row], &[usize]) {
        match form {
            DiffForm::SideBySide => (&diff.side, &diff.side_changes),
            DiffForm::Unified => (&diff.unified, &diff.unified_changes),
        }
    }

    /// Rebuilds the shown rows after a change of form, options or folds.
    fn refresh(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let Some(ready) = self.ready() else {
            self.shown.clear();
            self.positions.clear();
            return;
        };
        let (rows, _) = Self::rows_and_changes(&ready.diff, self.form);
        let shown = if self.fold {
            fold(rows, &ready.diff, &self.open)
        } else {
            (0..rows.len()).map(Shown::Row).collect()
        };
        self.shown = shown;
        self.positions = self.change_positions();
    }

    /// Where each change's first row is among the shown rows.
    fn change_positions(&self) -> Vec<usize> {
        let Some(ready) = self.ready() else {
            return Vec::new();
        };
        let (_, changes) = Self::rows_and_changes(&ready.diff, self.form);
        let mut positions = Vec::with_capacity(changes.len());
        let mut next = changes.iter().peekable();
        for (i, s) in self.shown.iter().enumerate() {
            let Shown::Row(r) = s else { continue };
            while let Some(&&c) = next.peek() {
                if c < *r {
                    next.next();
                } else {
                    break;
                }
            }
            if next.peek() == Some(&r) {
                positions.push(i);
                next.next();
            }
        }
        positions
    }

    fn set_form(&mut self, form: DiffForm) {
        if self.form != form {
            self.form = form;
            self.dirty = true;
            self.selection = None;
            self.jump = Some(self.current.unwrap_or(0));
        }
    }

    /// The fold button: off → on, on → off, and on with folds opened by hand → all folded.
    fn toggle_fold(&mut self, settings: &mut DiffWindowSettings) {
        if !(self.fold && !self.open.is_empty()) {
            self.fold = !self.fold;
            settings.fold = self.fold;
        }
        self.open.clear();
        self.dirty = true;
        self.jump = Some(self.current.unwrap_or(0));
    }

    fn step(&mut self, forward: bool, changes: usize) {
        if changes == 0 {
            return;
        }
        let target = match (self.current, forward) {
            (None, true) => 0,
            (None, false) => return,
            (Some(c), true) => (c + 1).min(changes - 1),
            (Some(c), false) => c.saturating_sub(1),
        };
        self.jump = Some(target);
    }

    /// Esc closes; Ctrl+D switches the form; Ctrl+Down/Up and F7/Shift+F7 move between changes.
    fn handle_keys(&mut self, ui: &Ui, settings: &mut DiffWindowSettings) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let changes = self.positions.len();
        let (form, next, prev, close) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Key::D),
                i.consume_key(Modifiers::COMMAND, Key::ArrowDown)
                    || i.consume_key(Modifiers::NONE, Key::F7),
                i.consume_key(Modifiers::COMMAND, Key::ArrowUp)
                    || i.consume_key(Modifiers::SHIFT, Key::F7),
                i.key_pressed(Key::Escape),
            )
        });
        if form {
            let other = match self.form {
                DiffForm::SideBySide => DiffForm::Unified,
                DiffForm::Unified => DiffForm::SideBySide,
            };
            self.set_form(other);
            settings.form = other;
        }
        if next {
            self.step(true, changes);
        }
        if prev {
            self.step(false, changes);
        }
        if close {
            self.closed = true;
        }
    }

    fn contents(&mut self, ui: &mut Ui, settings: &mut DiffWindowSettings) {
        self.refresh();
        let c = colors(ui);
        self.toolbar(ui, settings, &c);
        self.header(ui, &c);
        let body = ui.available_rect_before_wrap();
        ui.painter().rect_filled(body, 0.0, c.pane);
        match &self.load {
            Load::Loading(_) => message(ui, body, "Loading…", ui.visuals().weak_text_color()),
            Load::Failed(e) => message(
                ui,
                body,
                &format!("Could not read the file: {e}"),
                c.removed,
            ),
            Load::Ready(ready) => match &ready.loaded.content {
                Content::Binary { old_size, new_size } => {
                    let text = format!(
                        "Binary file, not shown.   Old: {}   New: {}",
                        size_text(*old_size),
                        size_text(*new_size)
                    );
                    message(ui, body, &text, ui.visuals().text_color());
                }
                Content::Submodule { old, new } => {
                    let short = |o: &Option<Oid>| {
                        o.map_or("(none)".to_owned(), |o| o.short(self.repo.abbrev_len))
                    };
                    let text = format!("Submodule: {} → {}", short(old), short(new));
                    message(ui, body, &text, ui.visuals().text_color());
                }
                Content::Text { .. } => {
                    let mut child = ui.new_child(UiBuilder::new().max_rect(body));
                    child.set_clip_rect(body.intersect(ui.clip_rect()));
                    self.body(&mut child, body, &c);
                }
            },
        }
        ui.allocate_rect(body, Sense::hover());
    }

    /// Form, changes, folding, word mode and whitespace, left to right.
    fn toolbar(&mut self, ui: &mut Ui, settings: &mut DiffWindowSettings, c: &Colors) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), TOOLBAR), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
        let mut bar = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(vec2(10.0, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let ui = &mut bar;
        ui.spacing_mut().item_spacing.x = 4.0;

        let forms = [
            (DiffForm::SideBySide, glyphs::DIFF_SIDE_BY_SIDE),
            (DiffForm::Unified, glyphs::DIFF_UNIFIED),
        ];
        let picked = widgets::segmented(ui, self.form, &forms, |form, r| {
            let text = match form {
                DiffForm::SideBySide => "Side by side",
                DiffForm::Unified => "Unified",
            };
            widgets::tip(r, text, "Ctrl+D")
        });
        if let Some(form) = picked {
            self.set_form(form);
            settings.form = form;
        }
        ui.add_space(14.0);

        let changes = self.positions.len();
        let at = self.current;
        let prev = ui
            .add_enabled_ui(at.is_some_and(|c| c > 0), |ui| {
                widgets::tip(
                    widgets::icon_button(ui, glyphs::CHEVRON_UP, false),
                    "Previous change",
                    "Ctrl+Up",
                )
            })
            .inner;
        let next = ui
            .add_enabled_ui(changes > 0 && at.is_none_or(|c| c + 1 < changes), |ui| {
                widgets::tip(
                    widgets::icon_button(ui, glyphs::CHEVRON_DOWN, false),
                    "Next change",
                    "Ctrl+Down",
                )
            })
            .inner;
        if prev.clicked() {
            self.step(false, changes);
        }
        if next.clicked() {
            self.step(true, changes);
        }
        let label = match (changes, at) {
            (0, _) => "No changes".to_owned(),
            (n, Some(c)) => format!("Change {} of {n}", c + 1),
            (n, None) => format!("{n} change{}", if n == 1 { "" } else { "s" }),
        };
        ui.add_space(4.0);
        ui.label(
            RichText::new(label)
                .size(12.5)
                .color(ui.visuals().weak_text_color()),
        );
        ui.add_space(14.0);

        // Three states: off, on, and on with folds opened by hand, which a click folds again.
        let opened = self.fold && !self.open.is_empty();
        let r = fold_button(ui, self.fold, opened);
        let r = if opened {
            widgets::tip_explained(
                r,
                "Fold unchanged lines",
                "",
                "Some folds are open. Click to fold them all again.",
            )
        } else {
            widgets::tip_explained(
                r,
                "Fold unchanged lines",
                "",
                "Hide the unchanged stretches between changes; click a fold to open it.",
            )
        };
        if r.clicked() {
            self.toggle_fold(settings);
        }
        ui.add_space(14.0);

        ui.label(
            RichText::new("Words")
                .size(12.5)
                .color(ui.visuals().weak_text_color()),
        );
        let mut words = self.options.words;
        let items = WordMode::ALL.map(|m| (m, m.label()));
        widgets::text_segmented(ui, &mut words, &items);
        if words != self.options.words {
            self.options.words = words;
            settings.words = words;
        }
        ui.add_space(14.0);

        let spaces = [
            (Whitespace::Compare, glyphs::WHITESPACE_COMPARE),
            (Whitespace::IgnoreChanges, glyphs::WHITESPACE_IGNORE_CHANGES),
            (Whitespace::IgnoreAll, glyphs::WHITESPACE_IGNORE_ALL),
        ];
        let picked = widgets::segmented(ui, self.options.whitespace, &spaces, |ws, r| {
            let (title, body) = match ws {
                Whitespace::Compare => (
                    "Compare whitespace",
                    "Every space, tab and line ending counts.",
                ),
                Whitespace::IgnoreChanges => (
                    "Ignore whitespace changes",
                    "More or fewer blanks, and blanks at line ends (CRLF too), don't count. As git diff -b.",
                ),
                Whitespace::IgnoreAll => {
                    ("Ignore all whitespace", "No blank counts. As git diff -w.")
                }
            };
            widgets::tip_explained(r, title, "", body)
        });
        if let Some(ws) = picked {
            self.options.whitespace = ws;
            settings.whitespace = ws;
        }
        let _ = c;
    }

    /// Status and path, the counts on the right, and the notes below.
    fn header(&self, ui: &mut Ui, c: &Colors) {
        let notes: &[Note] = self.ready().map_or(&[], |r| r.notes.as_slice());
        let height = if notes.is_empty() { 34.0 } else { 54.0 };
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, ui.visuals().panel_fill);
        painter.hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, c.line));
        let weak = ui.visuals().weak_text_color();
        let text = ui.visuals().text_color();
        let (status, color) = match self.spec.status {
            FileStatus::Added => ("Added", c.added),
            FileStatus::Deleted => ("Deleted", c.removed),
            FileStatus::Renamed => ("Renamed", c.renamed),
            FileStatus::Copied => ("Copied", c.renamed),
            other => (other.name(), weak),
        };
        let mut job = LayoutJob::default();
        let small = FontId::proportional(12.5);
        let big = FontId::proportional(14.5);
        job.append(status, 0.0, TextFormat::simple(small.clone(), color));
        let old_path = self.spec.old.as_ref().map(|v| v.path.as_str());
        let new_path = self.spec.new.as_ref().map(|v| v.path.as_str());
        match (old_path, new_path) {
            (Some(a), Some(b)) if a != b => {
                job.append(a, 10.0, TextFormat::simple(big.clone(), text));
                job.append(
                    " → ",
                    0.0,
                    TextFormat::simple(FontId::monospace(14.0), weak),
                );
                job.append(b, 0.0, TextFormat::simple(big.clone(), text));
            }
            _ => job.append(
                self.spec.path(),
                10.0,
                TextFormat::simple(big.clone(), text),
            ),
        }
        let line1 = rect.top() + 17.0;
        let g = painter.layout_job(job);
        painter.galley(pos2(rect.left() + 12.0, line1 - g.size().y / 2.0), g, text);
        if let Some(ready) = self.ready()
            && matches!(ready.loaded.content, Content::Text { .. })
        {
            let mut counts = LayoutJob::default();
            counts.append(
                &format!("+{}", ready.diff.added),
                0.0,
                TextFormat::simple(small.clone(), c.added),
            );
            counts.append(
                &format!("−{}", ready.diff.removed),
                8.0,
                TextFormat::simple(small.clone(), c.removed),
            );
            let g = painter.layout_job(counts);
            painter.galley(
                pos2(rect.right() - 12.0 - g.size().x, line1 - g.size().y / 2.0),
                g,
                text,
            );
        }
        if !notes.is_empty() {
            let line = notes
                .iter()
                .map(Note::to_string)
                .collect::<Vec<_>>()
                .join("   ·   ");
            let g = painter.layout_job(with_arrows(&line, &small, c.note));
            painter.galley(pos2(rect.left() + 12.0, rect.top() + 34.0), g, c.note);
        }
    }

    /// The rows, the overview strip and the horizontal scrollbar.
    fn body(&mut self, ui: &mut Ui, full: Rect, c: &Colors) {
        // The toolbar, drawn just before, may have changed the form or the folding.
        self.refresh();
        let positions = self.positions.clone();
        let Load::Ready(ready) = &self.load else {
            return;
        };
        let diff = &ready.diff;
        let font = FontId::monospace(FONT_SIZE);
        let row_h = ui.fonts_mut(|f| f.row_height(&font)).ceil() + 3.0;
        let char_w = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
        let digits = (diff.old.len().max(diff.new.len()).max(1) as f32)
            .log10()
            .floor()
            + 1.0;
        let gutter = digits * char_w + 18.0;
        let form = self.form;
        let side = form == DiffForm::SideBySide;

        let mut top = full.top();
        if side {
            let bar = Rect::from_min_max(full.min, pos2(full.right() - OVERVIEW, top + PANE_TITLE));
            self.pane_titles(ui, bar, c);
            top = bar.bottom();
        }

        // Sideways: how much text fits, and how far it can scroll.
        let body_w = full.width() - OVERVIEW;
        let text_w = if side {
            body_w / 2.0 - gutter - MARKER
        } else {
            body_w - 2.0 * gutter - MARKER
        };
        let content_w = diff.widest as f32 * char_w + 24.0;
        let hmax = (content_w - text_w).max(0.0);
        let bottom = full.bottom() - if hmax > 0.0 { SCROLLBAR } else { 0.0 };
        let area = Rect::from_min_max(
            pos2(full.left(), top),
            pos2(full.right() - OVERVIEW, bottom),
        );

        let (rows, _) = Self::rows_and_changes(diff, form);
        let mut scroll = ScrollArea::vertical()
            .auto_shrink(false)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .id_salt(("diff", self.id, self.form_ix(), self.fold));
        if let Some(offset) = self.scroll_to.take() {
            scroll = scroll.vertical_scroll_offset(offset.max(0.0));
            self.pinned = None;
        }
        let jumped = self.jump.take().filter(|&k| k < positions.len());
        if let Some(&at) = jumped.and_then(|k| positions.get(k)) {
            scroll = scroll.vertical_scroll_offset((at as f32 - LEAD).max(0.0) * row_h);
            // Bring the change's first changed word into view sideways.
            self.hoff = match first_word_column(diff, rows, &self.shown, at) {
                Some(col) if col as f32 * char_w > text_w - 40.0 => {
                    (col as f32 * char_w - 80.0).clamp(0.0, hmax)
                }
                _ => 0.0,
            };
        }
        if ui.rect_contains_pointer(area) {
            let dx = ui.input(|i| i.smooth_scroll_delta.x);
            self.hoff -= dx;
        }
        self.hoff = self.hoff.clamp(0.0, hmax);

        let geometry = Geometry {
            row_h,
            gutter,
            hoff: self.hoff,
            font: font.clone(),
        };
        let shown = &self.shown;
        let selection = self.selection;
        let mut clicked: Option<Click> = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(area));
        child.set_clip_rect(area.intersect(ui.clip_rect()));
        child.spacing_mut().item_spacing = Vec2::ZERO;
        let out = scroll.show_rows(&mut child, row_h, shown.len(), |ui, range| {
            for i in range {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                match &shown[i] {
                    Shown::Fold(hidden) => {
                        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                        fold_row(ui, rect, hidden.len(), response.hovered(), c);
                        if response.clicked() {
                            clicked = fold_lines(rows, hidden).map(Click::Fold);
                        }
                    }
                    Shown::Row(r) => {
                        let row = rows[*r];
                        let old = row.old.map(|i| &diff.old[i as usize]);
                        let new = row.new.map(|i| &diff.new[i as usize]);
                        let halves = if side {
                            let mid = rect.center().x;
                            let left = Rect::from_x_y_ranges(rect.left()..=mid, rect.y_range());
                            let right = Rect::from_x_y_ranges(mid..=rect.right(), rect.y_range());
                            paint_line(
                                ui,
                                left,
                                old,
                                Numbers::One(Side::Old),
                                &geometry,
                                selection,
                                c,
                            );
                            paint_line(
                                ui,
                                right,
                                new,
                                Numbers::One(Side::New),
                                &geometry,
                                selection,
                                c,
                            );
                            ui.painter()
                                .vline(mid, rect.y_range(), Stroke::new(1.0, c.line));
                            [(left, Side::Old, row.old), (right, Side::New, row.new)]
                        } else {
                            // A same line shows its new text (they may differ in ignored whitespace).
                            let line = new.or(old);
                            paint_line(
                                ui,
                                rect,
                                line,
                                Numbers::Both(row.old, row.new),
                                &geometry,
                                selection,
                                c,
                            );
                            let a = Rect::from_min_size(rect.min, vec2(gutter, row_h));
                            let b = a.translate(vec2(gutter, 0.0));
                            [(a, Side::Old, row.old), (b, Side::New, row.new)]
                        };
                        if response.clicked()
                            && let Some(p) = response.interact_pointer_pos()
                        {
                            for (r, side, line) in halves {
                                let numbers = Rect::from_min_size(r.min, vec2(gutter, row_h));
                                if let Some(line) = line
                                    && numbers.contains(p)
                                {
                                    let shift = ui.input(|i| i.modifiers.shift);
                                    clicked = Some(Click::Line(side, line, shift));
                                }
                            }
                        }
                    }
                }
            }
        });

        match clicked {
            Some(Click::Fold(lines)) => {
                self.open.push(lines);
                self.dirty = true;
            }
            Some(Click::Line(side, line, extend)) => {
                self.selection = match self.selection {
                    Some(s) if extend && s.side == side => Some(Selection { end: line, ..s }),
                    _ => Some(Selection {
                        side,
                        anchor: line,
                        end: line,
                    }),
                };
            }
            None => {}
        }

        // The change in view: the one jumped to while the view stays put, else the last one
        // at or above the reading line (as far down as a jump puts a change).
        let offset = out.state.offset.y;
        if let Some(k) = jumped {
            self.current = Some(k);
            self.pinned = Some(offset);
        } else if self.pinned.is_none_or(|p| (p - offset).abs() > 0.5) {
            self.pinned = None;
            let first = (offset / row_h).round() as usize;
            self.current = positions
                .iter()
                .rposition(|&p| p <= first + LEAD as usize)
                .or((!positions.is_empty() && first == 0).then_some(0));
        }

        let strip = Rect::from_min_max(pos2(full.right() - OVERVIEW, top), full.max);
        let view = (
            out.state.offset.y,
            out.inner_rect.height(),
            out.content_size.y,
        );
        overview(ui, strip, diff, rows, &self.shown, side, view, row_h, c);
        // The overview scrolls too: click or drag to put that place in the middle.
        let response = ui.interact(
            strip,
            egui::Id::new(("diff-overview", self.id)),
            Sense::click_and_drag(),
        );
        if (response.clicked() || response.dragged())
            && let Some(p) = response.interact_pointer_pos()
            && out.content_size.y > out.inner_rect.height()
        {
            let scale = (strip.height() / self.shown.len().max(1) as f32).min(row_h);
            let row = (p.y - strip.top()) / scale;
            self.scroll_to = Some(row * row_h - out.inner_rect.height() / 2.0);
            ui.ctx().request_repaint();
        }
        if hmax > 0.0 {
            let track = Rect::from_min_max(
                pos2(full.left(), bottom),
                pos2(full.right() - OVERVIEW, full.bottom()),
            );
            self.hscrollbar(ui, track, text_w / content_w, hmax, c);
        }
    }

    fn pane_titles(&self, ui: &Ui, bar: Rect, c: &Colors) {
        let painter = ui.painter();
        painter.rect_filled(bar, 0.0, ui.visuals().panel_fill);
        painter.hline(bar.x_range(), bar.top() + 0.5, Stroke::new(1.0, c.line));
        painter.hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, c.line));
        let weak = ui.visuals().weak_text_color();
        let mid = bar.center().x;
        for (version, x) in [(&self.spec.old, bar.left()), (&self.spec.new, mid)] {
            let text = match version {
                Some(v) => self.version_title(v),
                None => "(no file)".to_owned(),
            };
            let g = painter.layout_no_wrap(text, FontId::proportional(12.0), weak);
            let clip = Rect::from_x_y_ranges(x..=x + bar.width() / 2.0 - 8.0, bar.y_range());
            painter.with_clip_rect(clip).galley(
                pos2(x + 10.0, bar.center().y - g.size().y / 2.0),
                g,
                weak,
            );
        }
    }

    /// `<short hash>  <subject>`, and the path where it differs from the other side's.
    fn version_title(&self, v: &Version) -> String {
        let short = v.rev.short(self.repo.abbrev_len);
        let subject = self
            .repo
            .lookup(&v.rev)
            .map(|c| self.repo.commit(c).subject.clone())
            .unwrap_or_default();
        format!("{short}   {subject}")
    }

    fn hscrollbar(&mut self, ui: &mut Ui, track: Rect, visible: f32, hmax: f32, c: &Colors) {
        ui.painter()
            .rect_filled(track, 0.0, ui.visuals().panel_fill);
        let thumb_w = (track.width() * visible).clamp(30.0, track.width());
        let room = (track.width() - thumb_w).max(1.0);
        let x = track.left() + room * (self.hoff / hmax);
        let thumb = Rect::from_min_size(pos2(x, track.top() + 2.0), vec2(thumb_w, SCROLLBAR - 4.0));
        let response = ui.interact(
            track,
            egui::Id::new(("diff-hbar", self.id)),
            Sense::click_and_drag(),
        );
        if response.dragged() {
            self.hoff = (self.hoff + response.drag_delta().x * hmax / room).clamp(0.0, hmax);
        } else if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            self.hoff = ((p.x - track.left() - thumb_w / 2.0) / room * hmax).clamp(0.0, hmax);
        }
        let fill = if response.hovered() || response.dragged() {
            c.thumb_hover
        } else {
            c.thumb
        };
        ui.painter().rect_filled(thumb, CornerRadius::same(3), fill);
    }

    /// The text of the selected lines, one per line, without markers.
    fn selected_text(&self) -> Option<String> {
        let s = self.selection?;
        let ready = self.ready()?;
        let lines = match s.side {
            Side::Old => &ready.diff.old,
            Side::New => &ready.diff.new,
        };
        let (a, b) = (s.anchor.min(s.end) as usize, s.anchor.max(s.end) as usize);
        let mut text = lines
            .get(a..=b.min(lines.len().saturating_sub(1)))?
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        text.push('\n');
        Some(text)
    }

    /// Shows the window; returns nothing, but sets `closed` when it was closed.
    fn show(
        &mut self,
        ctx: &egui::Context,
        settings: &mut DiffWindowSettings,
        window_theme: Option<egui::SystemTheme>,
        icon: &Arc<egui::IconData>,
    ) {
        let builder = egui::ViewportBuilder::default()
            .with_title(self.title())
            .with_app_id(crate::settings::APP_ID)
            .with_icon(icon.clone())
            .with_inner_size(self.size)
            .with_min_inner_size([520.0, 320.0]);
        let id = self.viewport_id();
        if self.title_theme.is_none() && !ctx.embed_viewports() {
            ctx.request_repaint();
        }
        if std::mem::take(&mut self.focus) && !ctx.embed_viewports() {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
        }
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            self.poll();
            if class != egui::ViewportClass::EmbeddedWindow {
                if self.title_theme != window_theme {
                    self.title_theme = window_theme;
                    if let Some(theme) = window_theme {
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::SetTheme(theme));
                    }
                }
                let (close, size) = ui.input(|i| {
                    (
                        i.viewport().close_requested(),
                        i.viewport().inner_rect.map(|r| r.size()),
                    )
                });
                if let Some(size) = size
                    && size.x > 0.0
                    && size.y > 0.0
                {
                    settings.size = [size.x, size.y];
                }
                // Keys go to the main window too when the diff is embedded in it.
                self.handle_keys(ui, settings);
                if close {
                    self.closed = true;
                }
            }
            if ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)))
                && let Some(text) = self.selected_text()
            {
                ui.ctx().copy_text(text);
            }
            egui::CentralPanel::default()
                .frame(egui::Frame::central_panel(&ui.ctx().global_style()).inner_margin(0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    self.contents(ui, settings);
                });
        });
    }
}

enum Click {
    /// A fold: the lines of the new version it hides.
    Fold(std::ops::Range<u32>),
    /// A line number: the side, the line, and whether Shift was held.
    Line(Side, u32, bool),
}

/// Sizes shared by the rows of a frame.
struct Geometry {
    row_h: f32,
    gutter: f32,
    hoff: f32,
    font: FontId,
}

/// The line numbers a row shows: its side's, or both (unified).
enum Numbers {
    One(Side),
    Both(Option<u32>, Option<u32>),
}

/// Paints one line (or a filler, for `None`) into `rect`: line numbers, marker, text.
fn paint_line(
    ui: &Ui,
    rect: Rect,
    line: Option<&DiffLine>,
    numbers: Numbers,
    g: &Geometry,
    selection: Option<Selection>,
    c: &Colors,
) {
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let Some(line) = line else {
        painter.rect_filled(rect, 0.0, c.filler);
        return;
    };
    let (bg, word, marker, marker_color) = match line.kind {
        LineKind::Same => (Color32::TRANSPARENT, Color32::TRANSPARENT, "", c.weak),
        LineKind::Removed => (c.removed_line, c.removed_word, "−", c.removed),
        LineKind::Added => (c.added_line, c.added_word, "+", c.added),
    };
    painter.rect_filled(rect, 0.0, bg);
    let y = rect.top() + 1.5;
    let number = |x: f32, side: Side, no: Option<u32>| {
        let Some(ix) = no else { return };
        let selected = selection.is_some_and(|s| s.contains(side, ix));
        let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(g.gutter, g.row_h));
        if selected {
            painter.rect_filled(cell, 0.0, c.selected_bg);
        }
        let color = if selected { c.selected_fg } else { c.weak };
        let text = (ix + 1).to_string();
        let galley = painter.layout_no_wrap(text, g.font.clone(), color);
        painter.galley(pos2(x + g.gutter - 8.0 - galley.size().x, y), galley, color);
    };
    let mut x = rect.left();
    match numbers {
        Numbers::One(side) => {
            number(x, side, Some(line.no - 1));
            x += g.gutter;
        }
        Numbers::Both(old, new) => {
            number(x, Side::Old, old);
            x += g.gutter;
            number(x, Side::New, new);
            x += g.gutter;
        }
    }
    if !marker.is_empty() {
        let m = painter.layout_no_wrap(marker.into(), g.font.clone(), marker_color);
        painter.galley(pos2(x + 2.0, y), m, marker_color);
    }
    x += MARKER;
    let mut job = LayoutJob::default();
    let format = |background: Color32| TextFormat {
        font_id: g.font.clone(),
        color: c.text,
        background,
        ..Default::default()
    };
    let mut at = 0;
    for span in &line.spans {
        if span.start > at {
            job.append(
                &line.text[at..span.start],
                0.0,
                format(Color32::TRANSPARENT),
            );
        }
        job.append(&line.text[span.clone()], 0.0, format(word));
        at = span.end;
    }
    if at < line.text.len() {
        job.append(&line.text[at..], 0.0, format(Color32::TRANSPARENT));
    }
    let galley = painter.layout_job(job);
    let clip = Rect::from_min_max(pos2(x, rect.top()), rect.max).intersect(ui.clip_rect());
    painter
        .with_clip_rect(clip)
        .galley(pos2(x - g.hoff, y), galley, c.text);
}

/// The fold toggle: off, on, or on with some folds opened by hand (shown half on, with a dot).
fn fold_button(ui: &mut Ui, on: bool, opened: bool) -> egui::Response {
    let response = widgets::icon_button(ui, glyphs::FOLD, on && !opened);
    if opened {
        let t = widgets::tones(ui);
        let rect = response.rect;
        let painter = ui.painter();
        painter.rect_stroke(
            rect.shrink(0.5),
            CornerRadius::same(7),
            Stroke::new(1.0, t.on_fg.gamma_multiply(0.6)),
            StrokeKind::Inside,
        );
        painter.circle_filled(rect.right_top() + vec2(-6.0, 6.0), 3.0, t.on_fg);
    }
    response
}

/// A fold: `n unchanged lines`, across the row.
fn fold_row(ui: &Ui, rect: Rect, lines: usize, hovered: bool, c: &Colors) {
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, if hovered { c.fold_hover } else { c.fold });
    painter.hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, c.line));
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, c.line),
    );
    let text = if hovered {
        format!("Show {lines} unchanged lines")
    } else {
        format!("{lines} unchanged lines")
    };
    let color = if hovered {
        ui.visuals().text_color()
    } else {
        c.weak
    };
    let g = painter.layout_no_wrap(text, FontId::proportional(12.0), color);
    painter.galley(rect.center() - g.size() / 2.0, g, color);
}

/// The overview strip: every change's place in the whole diff, and the part in view.
#[allow(clippy::too_many_arguments)]
fn overview(
    ui: &Ui,
    strip: Rect,
    diff: &FileDiff,
    rows: &[Row],
    shown: &[Shown],
    side: bool,
    (offset, height, content): (f32, f32, f32),
    row_h: f32,
    c: &Colors,
) {
    let painter = ui.painter();
    painter.rect_filled(strip, 0.0, ui.visuals().panel_fill);
    painter.vline(
        strip.left() + 0.5,
        strip.y_range(),
        Stroke::new(1.0, c.line),
    );
    // A diff shorter than the window is drawn at its own scale, level with its rows.
    let n = shown.len().max(1) as f32;
    let scale = (strip.height() / n).min(row_h);
    let mark = |i: usize, kind: LineKind| {
        let y = strip.top() + i as f32 * scale;
        let color = if kind == LineKind::Removed {
            c.removed
        } else {
            c.added
        };
        let (x0, x1) = match (side, kind) {
            (true, LineKind::Removed) => (strip.left() + 3.0, strip.center().x),
            (true, _) => (strip.center().x, strip.right() - 2.0),
            _ => (strip.left() + 3.0, strip.right() - 2.0),
        };
        painter.rect_filled(
            Rect::from_min_max(pos2(x0, y), pos2(x1, y + scale.max(2.0))),
            0.0,
            color,
        );
    };
    for (i, s) in shown.iter().enumerate() {
        let Shown::Row(r) = s else { continue };
        let row = rows[*r];
        let kind = |lines: &[DiffLine], ix: Option<u32>| ix.map(|ix| lines[ix as usize].kind);
        if kind(&diff.old, row.old) == Some(LineKind::Removed) {
            mark(i, LineKind::Removed);
        }
        if kind(&diff.new, row.new) == Some(LineKind::Added) {
            mark(i, LineKind::Added);
        }
    }
    if content > height {
        let y0 = strip.top() + offset / content * strip.height();
        let y1 = strip.top() + (offset + height) / content * strip.height();
        painter.rect_stroke(
            Rect::from_min_max(
                pos2(strip.left() + 1.5, y0),
                pos2(strip.right() - 0.5, y1.min(strip.bottom())),
            ),
            1.0,
            Stroke::new(1.0, c.weak),
            StrokeKind::Inside,
        );
    }
}

/// The column of the first changed word in the shown row `at`, if any.
fn first_word_column(diff: &FileDiff, rows: &[Row], shown: &[Shown], at: usize) -> Option<usize> {
    let Some(Shown::Row(r)) = shown.get(at) else {
        return None;
    };
    let row = rows[*r];
    [
        row.old.map(|i| &diff.old[i as usize]),
        row.new.map(|i| &diff.new[i as usize]),
    ]
    .into_iter()
    .flatten()
    .filter_map(|l| l.spans.first().map(|s| l.text[..s.start].chars().count()))
    .min()
}

/// `text` in `font`, but its arrows in the monospace font: the proportional one has none.
fn with_arrows(text: &str, font: &FontId, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (i, part) in text.split('→').enumerate() {
        if i > 0 {
            job.append(
                "→",
                0.0,
                TextFormat::simple(FontId::monospace(font.size), color),
            );
        }
        job.append(part, 0.0, TextFormat::simple(font.clone(), color));
    }
    job
}

fn message(ui: &Ui, body: Rect, text: &str, color: Color32) {
    let g = ui
        .painter()
        .layout_job(with_arrows(text, &FontId::proportional(14.0), color));
    ui.painter().galley(body.min + vec2(20.0, 20.0), g, color);
}

fn size_text(size: Option<u64>) -> String {
    let Some(n) = size else {
        return "(none)".to_owned();
    };
    if n < 1024 {
        format!("{n} bytes")
    } else if n < 1024 * 1024 {
        format!("{:.1} KiB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", n as f64 / 1024.0 / 1024.0)
    }
}

struct Colors {
    pane: Color32,
    line: Color32,
    weak: Color32,
    text: Color32,
    filler: Color32,
    fold: Color32,
    fold_hover: Color32,
    removed_line: Color32,
    removed_word: Color32,
    added_line: Color32,
    added_word: Color32,
    removed: Color32,
    added: Color32,
    renamed: Color32,
    note: Color32,
    selected_bg: Color32,
    selected_fg: Color32,
    thumb: Color32,
    thumb_hover: Color32,
}

/// The log window's colours for panes and statuses, and tints for the lines.
fn colors(ui: &Ui) -> Colors {
    let t = widgets::tones(ui);
    let weak = ui.visuals().weak_text_color();
    let text = ui.visuals().text_color();
    if ui.visuals().dark_mode {
        Colors {
            pane: Color32::from_gray(22),
            line: Color32::from_white_alpha(23),
            weak,
            text,
            filler: Color32::from_gray(30),
            fold: Color32::from_gray(28),
            fold_hover: Color32::from_gray(36),
            removed_line: Color32::from_rgb(0x3d, 0x1c, 0x20),
            removed_word: Color32::from_rgb(0x80, 0x2c, 0x35),
            added_line: Color32::from_rgb(0x17, 0x33, 0x21),
            added_word: Color32::from_rgb(0x26, 0x62, 0x37),
            removed: Color32::from_rgb(0xff, 0x8a, 0x80),
            added: Color32::from_rgb(0x7b, 0xd8, 0x8f),
            renamed: Color32::from_rgb(0xd1, 0xa5, 0xff),
            note: Color32::from_rgb(0xe8, 0xc0, 0x6a),
            selected_bg: t.on_bg,
            selected_fg: Color32::from_rgb(0xcf, 0xe5, 0xff),
            thumb: Color32::from_white_alpha(50),
            thumb_hover: Color32::from_white_alpha(90),
        }
    } else {
        Colors {
            pane: Color32::WHITE,
            line: Color32::from_black_alpha(26),
            weak,
            text,
            filler: Color32::from_gray(243),
            fold: Color32::from_rgb(0xf3, 0xf6, 0xfa),
            fold_hover: Color32::from_rgb(0xe6, 0xee, 0xf8),
            removed_line: Color32::from_rgb(0xff, 0xeb, 0xe9),
            removed_word: Color32::from_rgb(0xff, 0xc0, 0xc0),
            added_line: Color32::from_rgb(0xe6, 0xff, 0xec),
            added_word: Color32::from_rgb(0xab, 0xf2, 0xbc),
            removed: Color32::from_rgb(0xc6, 0x28, 0x28),
            added: Color32::from_rgb(0x2e, 0x7d, 0x32),
            renamed: Color32::from_rgb(0x7b, 0x3f, 0xc4),
            note: Color32::from_rgb(0x9a, 0x5b, 0x00),
            selected_bg: t.on_bg,
            selected_fg: Color32::from_rgb(0x0b, 0x3d, 0x7a),
            thumb: Color32::from_black_alpha(45),
            thumb_hover: Color32::from_black_alpha(90),
        }
    }
}

impl ParterreApp {
    /// Every open diff window.
    pub(super) fn diff_windows(&mut self, ctx: &egui::Context) {
        let settings = &mut self.settings.diff_window;
        for window in &mut self.diffs.windows {
            window.show(ctx, settings, self.window_theme, &self.window_icon);
        }
        self.diffs.windows.retain(|w| !w.closed);
    }

    /// Opens the diff of `path` in the commit `rev` (a ref or hash prefix) against its first
    /// parent, for `--demo-diff`.
    pub(super) fn open_demo_diff(&mut self, spec: &str, ctx: &egui::Context) {
        let Some(repo) = self.repo.clone() else {
            return;
        };
        let Some((rev, path)) = spec.split_once(':') else {
            eprintln!("--demo-diff: expected <commit>:<path>");
            return;
        };
        let Some(ix) = repo.resolve(rev) else {
            eprintln!("--demo-diff: no commit named {rev}");
            return;
        };
        let commit = repo.commit(ix);
        let git = parterre_core::git::Git::new(&repo.path);
        let files = match git.changed_files(&commit.oid) {
            Ok(files) => files,
            Err(e) => {
                eprintln!("--demo-diff: {e}");
                return;
            }
        };
        let Some(file) = files.iter().find(|f| f.path == path) else {
            eprintln!("--demo-diff: {path} is not among the files {rev} changed");
            return;
        };
        let parent = commit.parents.first().map(|&p| repo.commit(p).oid);
        let spec = FileDiffSpec::of_commit(commit.oid, parent, file);
        self.diffs
            .open(repo.clone(), spec, &self.settings.diff_window, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parterre_core::file_diff::Version;
    use parterre_core::repo::Head;

    /// A window with a loaded diff of `old` against `new`, without git.
    fn window(old: &str, new: &str, settings: &DiffWindowSettings) -> DiffWindow {
        let rev = Oid::from_hex("0123456789012345678901234567890123456789").unwrap();
        let v = || Version {
            rev,
            path: "a.txt".into(),
        };
        let spec = FileDiffSpec {
            old: Some(v()),
            new: Some(v()),
            status: FileStatus::Modified,
            modes: [0o100644; 2],
            binary: false,
        };
        let loaded = LoadedDiff {
            spec: spec.clone(),
            content: Content::Text {
                old: old.into(),
                new: new.into(),
                invalid_bytes: 0,
            },
            textconv: None,
        };
        let repo = Arc::new(Repo::new(
            "/nowhere".into(),
            Vec::new(),
            Vec::new(),
            Head::Branch {
                name: "main".into(),
                target: None,
            },
        ));
        let (_, rx) = mpsc::channel();
        let mut w = DiffWindow::new_loading(1, repo, spec, settings, rx);
        let options = w.options;
        w.load = Load::Ready(Box::new(Ready::new(loaded, options)));
        w
    }

    /// Runs one frame of the window's contents with `events`.
    fn frame(
        ctx: &egui::Context,
        w: &mut DiffWindow,
        settings: &mut DiffWindowSettings,
        events: Vec<egui::Event>,
    ) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0))),
            events,
            ..Default::default()
        };
        // As `show` does, less the viewport. Nothing is rendered, so the texture updates are
        // discarded.
        w.poll();
        ctx.run_ui(input, |ui| w.contents(ui, settings))
            .textures_delta
            .clear();
    }

    fn click(at: egui::Pos2) -> [Vec<egui::Event>; 2] {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        [
            vec![egui::Event::PointerMoved(at), button(true)],
            vec![button(false)],
        ]
    }

    fn numbered(n: usize) -> String {
        (1..=n).map(|i| format!("line {i}\n")).collect()
    }

    #[test]
    fn switching_the_form_from_the_toolbar_draws_the_new_form() {
        // Unified has a row more than side by side: the edited line's two versions.
        let old = numbered(40);
        let new = old.replace("line 20\n", "line twenty\n");
        let mut settings = DiffWindowSettings {
            form: DiffForm::Unified,
            fold: false,
            ..DiffWindowSettings::default()
        };
        let mut w = window(&old, &new, &settings);
        let ctx = egui::Context::default();
        frame(&ctx, &mut w, &mut settings, Vec::new());
        // The side-by-side segment is the first in the toolbar.
        for events in click(pos2(27.0, TOOLBAR / 2.0)) {
            frame(&ctx, &mut w, &mut settings, events);
        }
        frame(&ctx, &mut w, &mut settings, Vec::new());
        assert_eq!(w.form, DiffForm::SideBySide);
        assert_eq!(settings.form, DiffForm::SideBySide);
    }

    fn folds(w: &DiffWindow) -> usize {
        w.shown
            .iter()
            .filter(|s| matches!(s, Shown::Fold(_)))
            .count()
    }

    #[test]
    fn opened_folds_survive_options_until_the_fold_button_folds_them_again() {
        let old = numbered(40);
        let new = old.replace("line 20\n", "line twenty\n");
        let mut settings = DiffWindowSettings::default();
        let mut w = window(&old, &new, &settings);
        let ctx = egui::Context::default();
        frame(&ctx, &mut w, &mut settings, Vec::new());
        assert_eq!(folds(&w), 2);

        // Open the first fold, as a click on it does.
        let Some(Shown::Fold(hidden)) = w.shown.first().cloned() else {
            panic!("expected a fold first: {:?}", w.shown);
        };
        let ready = w.ready().unwrap();
        let lines = fold_lines(&ready.diff.side, &hidden).unwrap();
        w.open.push(lines);
        w.dirty = true;
        frame(&ctx, &mut w, &mut settings, Vec::new());
        assert_eq!(folds(&w), 1);

        // Another whitespace setting and the other form keep it open.
        w.options.whitespace = Whitespace::IgnoreAll;
        w.set_form(DiffForm::Unified);
        frame(&ctx, &mut w, &mut settings, Vec::new());
        assert_eq!(w.ready().unwrap().options.whitespace, Whitespace::IgnoreAll);
        assert_eq!(folds(&w), 1);

        // The button, now in its third state, folds everything again and stays on.
        w.toggle_fold(&mut settings);
        frame(&ctx, &mut w, &mut settings, Vec::new());
        assert!(w.fold && w.open.is_empty());
        assert_eq!(folds(&w), 2);

        // Then it turns folding off.
        w.toggle_fold(&mut settings);
        frame(&ctx, &mut w, &mut settings, Vec::new());
        assert!(!w.fold && !settings.fold);
        assert_eq!(folds(&w), 0);
    }
}
