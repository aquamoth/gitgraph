# Architecture

gitgraph is a Cargo workspace with two crates:

```
crates/gitgraph-core   GUI-free; everything testable lives here
  git.rs               run `git log` / `git for-each-ref`, parse into a Repo
  repo.rs              Repo snapshot: commits (with parent indices), refs, HEAD
  revgraph.rs          reduce the commit DAG to a revision graph (TortoiseGit's rules)
  layout/              layered (Sugiyama) layout
    rank.rs            layer assignment (network simplex / longest path / chronological),
                       plus splitting of over-wide layers
    layered.rs         dummy items for long edges, optional edge bundling
    order.rs           crossing minimisation (median sweeps, exact crossing count)
    position.rs        coordinates within layers (L1 via isotonic regression)
    mod.rs             pipeline, variable layer spacing, direction/rotation
  physics.rs           the "spider web": springs + position-based dynamics for dragging

crates/gitgraph        the binary (eframe/egui)
  main.rs              CLI (clap), window setup
  app.rs               menus, toolbar, canvas interaction, search, status bar
  scene.rs             node contents and sizes + layout + physics net, hit testing
  render.rs            painting nodes, edges, arrows, overview
  view.rs              pan/zoom transform
  theme.rs             TortoiseGit colours (light, and dark via lightness inversion)
  settings.rs          persisted settings and the Classic/Modern looks
  automation.rs        --screenshot / --demo-drag scripted runs
```

## Data flow

1. **Load** (`git.rs`): one `git log --all` (notes excluded) with a compact
   `\x1f`/`\x1e`-separated format, one `git for-each-ref`, and HEAD queries. About 100 ms for
   15k commits. The snapshot holds *all* commits so view options never need git again.
2. **Reduce** (`revgraph.rs`): pick visible refs → reachable commits → decide which commits
   are nodes in one parents-first pass, recording for each hidden commit the node that
   represents it. Edges go from each node to the representatives of its parents.
   - *Labelled commits* reproduces `git log --simplify-by-decoration`, including
     `simplify_merges` (redundant parents dropped) and empty-tree roots (TREESAME).
     The node sets are identical on the 15k-commit Apps repository and on 400 random
     repositories, and the edges match on 300 of them.
     - One deliberate exception: git hides an empty-tree root even when it carries a label;
       gitgraph shows it.
   - *Branchings and merges* reproduces TortoiseGit's chain collapse.
3. **Measure** (`scene.rs`): node boxes use TortoiseGit's geometry: one row per ref, or an
   8-digit hash; 20 px side margins and 5 px top and bottom margins; monospace 12 px.
4. **Lay out** (`layout/`): rank → split wide layers → layered graph with dummies (optionally
   bundled per parent) → crossing minimisation → L1 coordinates → variable layer gaps →
   rotate to the chosen direction.
5. **Simulate** (`physics.rs`): nodes and bend points become particles. Springs run along
   edges, one-sided springs join neighbours in a layer, and weak anchors hold each particle to
   the layout.
   - **Shape:** each frame, the target shape is relaxed with Gauss-Seidel over displacements.
   - **Motion:** particles follow the target through damped springs.
   - Only the dragged node's neighbourhood (up to 8000 particles) is simulated, and the
     simulation sleeps when still.
6. **Paint** (`render.rs`): edges then nodes, culled to the viewport; text is skipped below
   4 px. Straight edges are clipped to box borders (TortoiseGit); curved edges leave and
   enter along the history direction.

## Testing

- Unit tests next to the code, plus integration tests (`crates/gitgraph-core/tests/`) that
  build throwaway repositories with the git CLI.
- Property tests (`properties_*.rs`) run random DAGs, repositories and drags against the
  invariants:
  - every parent is below its children
  - coordinates are finite and inside the bounds
  - network simplex is optimal on tiny graphs (checked by brute force)
  - every edge ends on a node
  - the net returns home after a reset
- `cargo test --release -p gitgraph-core --test properties_layout -- --ignored --nocapture`
  prints timings for large, awkward inputs.

## Why these choices

- **Rust + egui/eframe**: native speed, one codebase for Linux, Windows and macOS, and an
  immediate-mode canvas that makes custom drawing and dragging simple. No system development
  packages are needed to build on Linux (winit/glutin load Wayland/X11/GL at runtime).
- **git CLI instead of a git library**: always available where gitgraph is useful, honours
  every repo configuration, fast enough (see above), and keeps the build free of C
  dependencies.
- **Own layout instead of a graph-layout crate**: git-specific needs (first-parent weighting,
  TortoiseGit parity, layer splitting, bundling, anytime network simplex) and full control
  over performance. 15k nodes lay out in about 200 ms.
