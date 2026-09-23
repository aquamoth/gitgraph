# gitgraph

A standalone, fast, native re-creation of TortoiseGit's **Revision Graph**: a compact,
tree-like picture of how the branches and tags of a git repository relate, in a resizable window
that runs on Linux and Windows (and should run on macOS).

On top of the TortoiseGit look you can grab any node and drag it; the rest of the graph follows
like a spider web being pulled.

## Usage

```sh
gitgraph [PATH]        # open the repository containing PATH (default: current directory)
gitgraph --help        # all options
```

gitgraph needs `git` on `PATH` at runtime; it reads the repository with `git log` and
`git for-each-ref` and never writes to it.

## Building

Requires a stable Rust toolchain (install with [rustup](https://rustup.rs)).

```sh
cargo build --release          # binary: target/release/gitgraph
cargo test --workspace         # unit + integration tests (need git on PATH)
cargo clippy --workspace --all-targets
```

On Linux the window uses Wayland or X11 through `winit`; no extra development packages are
needed to build. See [docs/building.md](docs/building.md) for Windows and macOS notes.

## Layout of the repository

| Path | What |
|---|---|
| `crates/gitgraph-core` | GUI-free core: git loading, revision-graph reduction, layered layout, drag physics |
| `crates/gitgraph` | The `gitgraph` binary: egui/eframe window, rendering, interaction |
| `docs/research/` | Notes on how TortoiseGit's revision graph works, with source links |
| `docs/architecture.md` | How the pieces fit together |
| `TODO.md` | Open questions and planned work |
