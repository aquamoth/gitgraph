# TODO

## Open questions (HITL)

_Decisions I made on my own that you may want to overrule. Try them with `gitgraph` on
`~/Source/repos/Cosmo/Apps`; most are one click in the toolbar or menus._

1. **Default look: "Modern" or "Classic"?** The toolbar has a Look selector.
   - **Classic** is TortoiseGit: straight edges, every edge drawn separately, rows as wide as
     needed.
   - **Modern** (current default) uses curved edges and bundles edges that run into the same
     commit into one trunk. It also splits rows wider than 1800 px so sibling branches stack.

   On Apps, ~160 remote branches hang off a few commits. In Classic that gives rows many
   thousands of pixels wide with fans of near-horizontal lines. Modern reads like a tree.
   Which do you want by default?
2. **Dragging feel.** Menu *Drag* has three prototypes:
   - **Spider web** (default): springs along edges and between neighbours in a row. The rest of
     the graph follows with some inertia and wobble.
   - **Strings**: springs along edges only, no wobble.
   - **Rigid**: only the dragged node moves.

   The *reach* and *wobble* sliders tune the first two. A dropped node stays pinned (blue dot);
   right-click → "Return node to layout", or `R` for all nodes. Which feels right, and should
   moved positions be remembered per repository between runs?
3. **Fidelity quirks in "Labelled commits" (TortoiseGit's default mode).** TortoiseGit uses
   `git log --simplify-by-decoration` and inherits git's simplifications:
   - A `--no-ff` merge whose first parent is an ancestor of its second is folded away.
   - A merge that brings in a history whose root has an *empty tree* (svn imports,
     `--allow-empty` first commits) is folded away.

   gitgraph copies this exactly; its node set for Apps is identical to git's (266 nodes).
   "Branchings and merges" and "All commits" show the real topology. Keep the fidelity?
4. **Initial view.** As in TortoiseGit, the window opens at 100% with HEAD near the top. On
   big graphs that shows only a small area. Would fit-to-window, or a fixed zoom such as 60%,
   be better?
5. **Layer spacing.** Gaps between rows grow when long sideways edges cross them. TortoiseGit
   (OGDF) does the same, capped at 300 px. This keeps edges steep but makes the graph taller.
   Tune it under Graph → Spacing. Happy with the default?
6. **Stash** is shown, as in TortoiseGit, as a single edge to its base commit. The index and
   untracked-files snapshot commits are hidden.
7. **`origin/HEAD`**-style symbolic refs are hidden, because they duplicate `origin/main`.
   TortoiseGit shows them.
8. **Other refs** (`refs/t3/*` in Apps) are hidden by default; Graph → Other refs shows them.
9. **HEAD marker.** Like TortoiseGit, only the current branch's row is highlighted (red). A
   detached HEAD gets its own red "HEAD" row, which TortoiseGit doesn't have.
10. **No git actions.** Per your brief, there's no checkout, log, diff or delete. The context
    menu only copies hashes, ref names or the subject. Should any actions be added?
11. **License?** None is chosen yet (e.g. MIT OR Apache-2.0).
12. **Windows builds.** The code type-checks for Windows. Producing an `.exe` from this
    machine needs a linker: `sudo apt install mingw-w64`, `cargo-zigbuild`, or `cargo-xwin`
    (which means accepting Microsoft's CRT license). Alternatively, build natively on Windows,
    or set up CI once the repo has a remote. Which do you prefer?
13. **Performance at 100k commits.** I measured this on a synthetic repository with 100k
    commits, 2,490 refs and 1,846 merges:
    - Loading takes 0.6 s.
    - "Labelled commits" (3.6k nodes) lays out in 0.15 s, "Branchings and merges" (7.3k nodes)
      in 0.2 s.
    - "All commits" takes 2.8 s on a background thread, because long-lived branches create
      1.5M bend points. Peak memory is then about 880 MB.
    - Dragging runs at 8 ms per frame.

    Is 2.8 s and 880 MB for the all-commits view of a 100k repo acceptable, or worth more
    work? (Apps, at 15k commits, needs 0.2 s.)

## Planned

- [ ] PNG export: SVG exists; TortoiseGit also offers raster formats.
- [ ] Remember dragged positions per repository, if wanted (question 2).
- [ ] Reload automatically when refs change; TortoiseGit only reloads on F5.
- [ ] Tooltip on edges showing the collapsed commits.
- [ ] Less memory for all-commits views of huge repositories (compact adjacency).
- [ ] Windows `.exe` icon resource; try on macOS.

## Done

- [x] Workspace scaffold, lints, release profile, docs (`docs/architecture.md`,
      `docs/building.md`).
- [x] Research into how TortoiseGit's revision graph works (`docs/research/`).
- [x] Git loading through the git CLI (about 100 ms for 15k commits).
- [x] Revision-graph reduction in TortoiseGit's modes, with integration tests; matches
      `git log --simplify-by-decoration` exactly on Apps.
- [x] Layered layout:
  - network-simplex ranking, median crossing reduction, L1 coordinates
  - variable layer spacing
  - splitting of over-wide rows
  - optional edge bundling
  - four directions
- [x] Window: TortoiseGit colours and node geometry, light and dark themes, straight or
      curved edges with arrows, pan and zoom, fit, go to HEAD, search, tooltips, context
      menu, overview map, persisted settings, status bar.
- [x] Draggable nodes with spider-web physics, in three models. The net's shape minimises
      spring and anchor energy over displacements; only the dragged node's neighbourhood is
      simulated. Pinning and reset.
- [x] Windows type-check (`cargo check --target x86_64-pc-windows-gnu`).
- [x] Crossing reduction with transposition and 12 restarts, as OGDF does: 16–30% fewer
      crossings.
- [x] Layout on a background thread, with the view kept anchored on the same commit.
- [x] SVG export from the menu, plus headless `--export out.svg`.
- [x] Filters: current branch only, and a ref-name filter.
- [x] Full commit messages in tooltips, loaded on demand.
- [x] Window icon drawn in code; Linux `.desktop` entry; pre-commit hook (fmt and clippy).
