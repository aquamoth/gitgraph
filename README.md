# parterre

A standalone, fast, native re-creation of TortoiseGit's **Revision Graph**: a compact,
tree-like picture of how the branches and tags of a git repository relate, in a resizable window
that runs on Linux and Windows (and should run on macOS).

A parterre is a formal garden laid out in patterns, designed above all to be seen from the upper
floors of the house. This parterre gives you that view of a repository: every branch at once,
from above. (Up to version 0.2 it was called gitgraph.)

On top of the TortoiseGit look you can rearrange the graph by hand. Drag a node and the rest of
the graph gives way a little: neighbours follow along their edges and nodes in the way move
aside, like weak magnets. Other drag modes move only the selected nodes, or a whole subtree.
Edges at moved nodes are routed afresh through the gaps between nodes, so they lose bends they
no longer need and go around nodes that are now in the way.

![parterre showing a demo repository](docs/images/demo.png)

_(Made with `scripts/make-demo-repo.sh`: local branches green, remote branches orange, tags
yellow, the current branch red.)_

## Installing

Download the archive for your system from the
[releases page](https://github.com/aquamoth/parterre/releases) and put `parterre` on your
`PATH`. On Windows the `.msi` next to it does that for you, and adds a Start menu entry; it
installs for the current user without asking for admin rights. The Linux build needs glibc 2.35 or newer (Debian 12, Ubuntu 22.04 and later). With a
Rust toolchain you can also install it from crates.io:

```sh
cargo install --locked parterre    # build from source; installs only the binary
cargo binstall parterre            # or download the release binary with cargo-binstall
```

parterre also needs `git`. Packages for winget, Chocolatey and Linux are on their way; see
[docs/distribution.md](docs/distribution.md).

## Usage

```sh
parterre [PATH]                    # open the repository containing PATH (default: the
                                   # current directory's, or none: the window asks for one)
parterre --mode branches           # also show every fork point and merge
parterre --mode all --no-remotes   # every commit, local branches and tags only
parterre --look classic            # straight, unbundled edges like TortoiseGit
parterre --hide 'pipeline/*,release/*'        # leave out build and release branches
parterre --branch-color 'feature/*=#9b59b6'   # colour branches by name (repeatable)
parterre --export graph.svg        # write an SVG without opening a window
parterre --help                    # all options
```

In the window:

| Do | To |
|---|---|
| Drag a node | Move it, with the rest of the selection it belongs to. In *Adapt*, the graph gives way and keeps children above their parents. |
| `1` / `2` / `3` | Drag mode *Adapt* (the graph gives way) / *Free* (nothing else moves) / *Subtree* (take along everything that grows out of it) |
| Click, `Ctrl`+click, `Shift`+click a node | Select it / toggle it / add it to the selection |
| `Shift`+drag the background | Select the nodes in a rectangle |
| Hover / click an edge | List the commits collapsed into it / keep it highlighted while you look around |
| `Ctrl+Z` / `Ctrl+Shift+Z` | Undo / redo a move |
| Drag the background, wheel, Shift+wheel | Pan |
| Ctrl+wheel, pinch, `+` `-` `0` | Zoom |
| `F`, double-click the background | Fit the whole graph |
| `Home` / `H` | Go to HEAD |
| `Ctrl+F`, then `Enter` / `F3` | Find branches, tags, hashes, subjects or authors |
| Right-click a node | Copy its hash, ref names or subject; select its subtree; return it to the layout |
| `R` | Return all nodes to the layout |
| `Esc` | Clear the selection |
| `F5` | Reload the repository |
| `Ctrl+O` / `Ctrl+W` | Open / close a folder; the ☰ menu also lists the recent ones |
| `Ctrl+,` | Settings |

The toolbar holds what you use every day, the ☰ menu has all of that and more, and
*Settings* (`Ctrl+,`) the rest; the graph shows every change while the settings stay open.
TortoiseGit's options are all there:
- show branchings and merges
- local or remote branches
- tags, and "show all tags"
- arrows pointing towards merges
- zoom, the overview map and export

parterre adds:
- four directions and three vertical placements
- edge bundling, row splitting and curved edges
- first-parent-only view, and stash or other refs
- hiding branches by wildcard, e.g. `pipeline/*` (the toolbar's filter options, or *Settings → Filters*). A hidden branch
  still shows where the history of a shown branch contains it, so only leaves vanish.
- colours by branch name, e.g. `feature/*` purple (*Settings → Branch colours*)
- light and dark themes
- rearranging by hand: drag modes, multi-selection, undo

Colours follow TortoiseGit:

| Label | Colour |
|---|---|
| Current branch | red |
| Local branches | green |
| Remote branches | light orange |
| Tags | yellow |
| Commits without refs | pale lavender, showing an 8-digit hash |

Colours chosen per branch name replace these, except for the current branch.

parterre needs `git` on `PATH` at runtime; it reads the repository with `git log` and
`git for-each-ref` and never writes to it.

## Building

Requires a stable Rust toolchain (install with [rustup](https://rustup.rs)).

```sh
cargo build --release          # binary: target/release/parterre
cargo test --workspace         # unit + integration tests (need git on PATH)
cargo clippy --workspace --all-targets
```

On Linux the window uses Wayland or X11 through `winit`; no extra development packages are
needed to build. See [docs/building.md](docs/building.md) for Windows and macOS notes, and
[docs/releasing.md](docs/releasing.md) for how releases and version numbers work.

## Layout of the repository

| Path | What |
|---|---|
| `crates/parterre-core` | GUI-free core: git loading, revision-graph reduction, layered layout, drag physics |
| `crates/parterre` | The `parterre` binary: egui/eframe window, rendering, interaction |
| `docs/research/` | Notes on how TortoiseGit's revision graph works, with source links |
| `docs/architecture.md` | How the pieces fit together |
| `docs/distribution.md` | Where parterre is published, under which names, and why |
| `TODO.md` | Open questions and planned work |

## License

parterre is free software under the [GNU General Public License, version 3](LICENSE) only, with
two additional terms in [NOTICE](NOTICE): works based on parterre keep its copyright notice and
say that they are based on it, and modified versions are marked as modified. You may use, share
and modify parterre at home and at work.

Release builds come with `THIRD-PARTY-NOTICES.html`, the licenses of the Rust crates they
contain. To generate it yourself, install [cargo-about](https://github.com/EmbarkStudios/cargo-about)
and run `cargo about generate -c packaging/about.toml packaging/about.hbs -o THIRD-PARTY-NOTICES.html`.

## Contributing

Bug reports and ideas are welcome as issues. Read [CONTRIBUTING.md](CONTRIBUTING.md) before
writing code for a pull request.
