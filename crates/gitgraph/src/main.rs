//! gitgraph: a standalone TortoiseGit-style revision graph viewer.

// Release builds on Windows are GUI-subsystem apps (no console window).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod automation;
mod export;
mod icon;
mod render;
mod scene;
mod settings;
mod theme;
mod view;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use eframe::egui;
use gitgraph_core::layout::Direction;
use gitgraph_core::revgraph::Simplification;

use crate::automation::Automation;
use crate::theme::ThemeChoice;

/// Show the revision graph of a git repository: how its branches and tags relate.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    /// Repository to show (any directory inside it).
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Which commits to show.
    #[arg(long, value_enum)]
    mode: Option<Mode>,

    /// Where the newest commits go.
    #[arg(long, value_enum)]
    direction: Option<Dir>,

    /// Overall look: "modern" (curved, bundled edges) or "classic" (as TortoiseGit).
    #[arg(long, value_enum)]
    look: Option<LookArg>,

    /// Maximum row width before siblings stack up (0 = unlimited, as TortoiseGit).
    #[arg(long)]
    max_row_width: Option<f32>,

    /// Show only the history of HEAD.
    #[arg(long)]
    current_branch: bool,

    /// Only branches and tags whose names contain one of these comma-separated words.
    #[arg(long, value_name = "WORDS")]
    filter: Option<String>,

    /// Hide remote-tracking branches.
    #[arg(long)]
    no_remotes: bool,

    /// Hide tags.
    #[arg(long)]
    no_tags: bool,

    /// Colour theme.
    #[arg(long, value_enum)]
    theme: Option<Theme>,

    /// Initial window size, e.g. 1600x1000.
    #[arg(long, value_parser = parse_size)]
    window_size: Option<(f32, f32)>,

    /// Write the graph as SVG to FILE and exit, without opening a window.
    #[arg(long, value_name = "FILE")]
    export: Option<PathBuf>,

    /// Render the window to a PNG file and exit (for testing and documentation).
    #[arg(long, value_name = "FILE")]
    screenshot: Option<PathBuf>,

    /// Start with the whole graph in view instead of at HEAD.
    #[arg(long)]
    fit: bool,

    /// Show the overview map.
    #[arg(long, hide = true)]
    overview: bool,

    /// Zoom level for the screenshot (1 = 100%), applied around the centre of the initial view.
    #[arg(long, hide = true)]
    zoom: Option<f32>,

    /// Drag the centre node by DX,DY before taking the screenshot (demonstrates the physics).
    #[arg(long, value_name = "DX,DY", value_parser = parse_vec, hide = true)]
    demo_drag: Option<(f32, f32)>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Mode {
    /// Commits with refs, and merges joining them (TortoiseGit default).
    Labelled,
    /// Also every fork point and merge (TortoiseGit "Show branchings and merges").
    Branches,
    /// Every commit.
    All,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum LookArg {
    Modern,
    Classic,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Dir {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Theme {
    System,
    Light,
    Dark,
}

fn parse_size(s: &str) -> Result<(f32, f32), String> {
    let (w, h) = s.split_once(['x', 'X']).ok_or("expected WIDTHxHEIGHT")?;
    Ok((
        w.parse().map_err(|_| "bad width")?,
        h.parse().map_err(|_| "bad height")?,
    ))
}

fn parse_vec(s: &str) -> Result<(f32, f32), String> {
    let (x, y) = s.split_once(',').ok_or("expected DX,DY")?;
    Ok((
        x.parse().map_err(|_| "bad DX")?,
        y.parse().map_err(|_| "bad DY")?,
    ))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let repo = match gitgraph_core::git::load_repo(&cli.path) {
        Ok(repo) => repo,
        Err(e) => {
            eprintln!("gitgraph: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(path) = cli.export.clone() {
        let mut settings = settings::Settings::default();
        apply_cli(&cli, &mut settings);
        return match export_headless(&std::sync::Arc::new(repo), &settings, &path) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("gitgraph: could not write {}: {e}", path.display());
                ExitCode::FAILURE
            }
        };
    }

    let (w, h) = cli.window_size.unwrap_or((1400.0, 900.0));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("{} – gitgraph", repo.display_name()))
            .with_app_id("gitgraph")
            .with_inner_size([w, h])
            .with_min_inner_size([400.0, 300.0])
            .with_icon(std::sync::Arc::new(icon::icon())),
        ..Default::default()
    };
    let automation = Automation::new(
        cli.screenshot.clone(),
        cli.fit,
        cli.demo_drag.map(|(x, y)| egui::vec2(x, y)),
        cli.zoom,
    );
    let path = cli.path.clone();
    let overrides = move |s: &mut settings::Settings| apply_cli(&cli, s);
    let result = eframe::run_native(
        "gitgraph",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::GitGraphApp::new(
                cc, path, repo, overrides, automation,
            )))
        }),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gitgraph: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Applies command-line options on top of the stored settings.
fn apply_cli(cli: &Cli, s: &mut settings::Settings) {
    if let Some(mode) = cli.mode {
        s.graph.simplification = match mode {
            Mode::Labelled => Simplification::Decorated,
            Mode::Branches => Simplification::BranchesAndMerges,
            Mode::All => Simplification::AllCommits,
        };
    }
    if let Some(dir) = cli.direction {
        s.layout.direction = match dir {
            Dir::Top => Direction::NewestTop,
            Dir::Bottom => Direction::NewestBottom,
            Dir::Left => Direction::NewestLeft,
            Dir::Right => Direction::NewestRight,
        };
    }
    match cli.look {
        Some(LookArg::Modern) => settings::Look::Modern.apply(s),
        Some(LookArg::Classic) => settings::Look::Classic.apply(s),
        None => {}
    }
    if let Some(w) = cli.max_row_width {
        s.layout.max_layer_width = w;
    }
    if cli.overview {
        s.show_overview = true;
    }
    if cli.current_branch {
        s.graph.current_branch_only = true;
    }
    if let Some(filter) = &cli.filter {
        s.graph.ref_filter = filter.clone();
    }
    if cli.no_remotes {
        s.graph.show_remote_branches = false;
    }
    if cli.no_tags {
        s.graph.show_tags = false;
    }
    if let Some(theme) = cli.theme {
        s.theme = match theme {
            Theme::System => ThemeChoice::System,
            Theme::Light => ThemeChoice::Light,
            Theme::Dark => ThemeChoice::Dark,
        };
    }
}

/// Lays the graph out without a window and writes it as SVG.
fn export_headless(
    repo: &std::sync::Arc<gitgraph_core::Repo>,
    settings: &settings::Settings,
    path: &std::path::Path,
) -> std::io::Result<()> {
    let ctx = egui::Context::default();
    // One pass initialises the fonts used to measure labels.
    // Nothing is rendered, so the texture updates are discarded.
    ctx.run_ui(egui::RawInput::default(), |_| {})
        .textures_delta
        .clear();
    let font = egui::FontId::monospace(scene::FONT_SIZE);
    let text_height = ctx.fonts_mut(|f| f.row_height(&font));
    let input = ctx.fonts_mut(|f| {
        let mut width = |s: &str| {
            f.layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE)
                .size()
                .x
        };
        scene::Scene::prepare(repo, settings, &mut width, text_height)
    });
    let scene = input.lay_out();
    let palette = match settings.theme {
        theme::ThemeChoice::Dark => theme::Palette::dark(),
        _ => theme::Palette::light(),
    };
    std::fs::write(path, export::to_svg(&scene, settings, &palette))?;
    eprintln!("wrote {} ({} nodes)", path.display(), scene.node_count());
    Ok(())
}
