# TODO

## Open questions (HITL)

_Decisions I made on my own that you may want to overrule. Try them with `parterre` on
`~/Source/repos/Cosmo/Apps`; most are one click in the toolbar or menus._

1. **Default look: "Modern" or "Classic"?** The toolbar has a Look selector.
   - **Classic** is TortoiseGit: straight edges, every edge drawn separately, rows as wide as
     needed.
   - **Modern** (current default) uses curved edges and bundles edges that run into the same
     commit into one trunk. It also splits rows wider than 1800 px so sibling branches stack.

   On Apps, ~160 remote branches hang off a few commits. In Classic that gives rows many
   thousands of pixels wide with fans of near-horizontal lines. Modern reads like a tree.
   Which do you want by default?
2. **Rearranging by hand.** Reworked from your notes of 2026-09-24:
   - A dropped node is no longer pinned. Wherever things come to rest becomes their new
     resting shape, so moved nodes keep giving way to later drags like any other node.
   - Three drag modes, in the toolbar, the *Drag* menu and on keys `1` `2` `3`:
     - **Adapt** (default): neighbours follow along their edges (*pull* slider), and nodes
       that come near are pushed aside like weak magnets (*push*). Nodes side by side in a row
       are pushed ahead along it; lifting a node out of the row lets it pass them.
     - **Free**: only the selected nodes move; the edges to them stretch.
     - **Subtree**: the selected nodes and everything that grows out of them. Hovering shows
       what would move.
   - Switching back to Adapt keeps every node where it is, and from then on the springs hold
     the new offsets between neighbours.
   - Selecting: click; Ctrl+click toggles; Shift+click adds; Shift+drag the background selects
     a rectangle; right-click → *Select subtree*. Dragging a selected node moves the whole
     selection.
   - Undo and redo with Ctrl+Z / Ctrl+Shift+Z; `R` (reset) can be undone too.
   - Edges re-route as you drag (your notes of 2026-09-24, second round): an edge at a node
     you moved takes a new route through the gaps between the rows it crosses. It bends only
     where it must, so it loses bends when its nodes come together and goes around the nodes
     in between when they move apart. Edges a moved node comes to cover make way too.

   Decisions you may want to overrule:
   - **Subtree follows first parents.** A commit's subtree is everything whose first-parent
     line leads back to it: its branch, and the branches forking off that. A merge that pulls
     the branch in belongs to the line it merged into, so it stays put. Taking every descendant
     instead would include everything merged later, often most of the graph.
   - **What moved stays moved.** Neighbours that Adapt pulls or pushes keep their new places
     after the drop. The alternative is for them to spring back, so that only the dragged node
     keeps its new place.
   - **Blue dots** mark only the nodes you grabbed. Nodes that gave way, or that a subtree
     carried along, get none (but can still be returned to the layout).
   - **Magnets act between nodes.** During a drag, edges make way only for nodes in their own
     row; once the node is dropped on them they re-route around it.
   - **Which edges re-route.** Edges at nodes you moved re-route as soon as they change.
     Edges that only gave way keep the layout's route (bent along) unless pulled more than
     40 px out of shape. Re-routing those too made whole fans lose their bundled trunks when a
     shared parent moved a pixel.
   - **Re-routed edges switch at once,** without animating from the old route to the new.
     They also leave the trunks that bundled edges share.
   - **Reversed edges.** An edge whose parent is dragged above its child now leaves the
     child's top and enters the parent's bottom, instead of looping round both.
   - **Free and Subtree allow overlaps,** and Adapt leaves them alone: it only keeps nodes
     from coming closer than they rest.
   - **Defaults:** pull 0.3 (a neighbour moves about half as far as the dragged node, the next
     one a quarter), push 0.5 (nodes start pushing each other 32 px apart), wobble 0.4. *Pull*
     replaces the old *reach* setting and starts at its new default.
   - **The old prototypes are gone.** Spider web and Strings became Adapt; Rigid became Free.
   - **Mode switching** uses keys and buttons only. Shift and Ctrl already mean selection;
     another modifier (such as holding Space) could give a one-off Free drag.
   - **Remembering moves.** Drag → *Remember moved nodes* (off by default) keeps nodes where
     they rest, per repository, across runs and relayouts. Should it be on by default?
3. **Fidelity quirks in "Labelled commits" (TortoiseGit's default mode).** TortoiseGit uses
   `git log --simplify-by-decoration` and inherits git's simplifications:
   - A `--no-ff` merge whose first parent is an ancestor of its second is folded away.
   - A merge that brings in a history whose root has an *empty tree* (svn imports,
     `--allow-empty` first commits) is folded away.

   parterre copies this exactly; its node set for Apps is identical to git's (266 nodes).
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
11. **Performance at 100k commits.** I measured this on a synthetic repository with 100k
    commits, 2,490 refs and 1,846 merges:
    - Loading takes 0.6 s.
    - "Labelled commits" (3.6k nodes) lays out in 0.15 s, "Branchings and merges" (7.3k nodes)
      in 0.2 s.
    - "All commits" takes 2.8 s on a background thread, because long-lived branches create
      1.5M bend points. Peak memory is then about 880 MB.
    - Dragging runs at 8 ms per frame.

    Is 2.8 s and 880 MB for the all-commits view of a 100k repo acceptable, or worth more
    work? (Apps, at 15k commits, needs 0.2 s.)
12. **Releases** (`docs/releasing.md`). Decisions you may want to overrule:
    - **Version in the UI:** besides `--version`, the Help menu ends with a greyed
      `parterre 0.3.0 (a1b2c3d)` line, for users who start parterre from a file manager or
      Start menu and never see a terminal.
    - **Assets:** one archive per target (`.tar.gz`, `.zip` on Windows) holding the binary,
      the README, `LICENSE`, `NOTICE` and `THIRD-PARTY-NOTICES.html`, plus `SHA256SUMS`. macOS
      gets both Apple silicon and Intel builds, the Intel one cross-compiled and therefore not
      test-run in the workflow.
    - **Linux baseline:** built on `ubuntu-latest`, so the binary needs that runner's glibc
      (2.39) or newer. Building on an older runner would reach older distributions.
    - **Commit detection** is a `build.rs` running `git`, with no dependencies. Without git it
      falls back to a bare `X.Y.Z-dev`. Only the release workflow can produce a plain version:
      a local build of a tagged commit still reads `-dev`.
    - **Dirty** means uncommitted changes under `crates/`, `.cargo/`, the Cargo files or
      `rust-toolchain.toml`, the files that go into the binary. Edits to docs don't count.
13. **Hiding and colouring branches by name** (your request of 2026-09-25). Neither is in
    TortoiseGit. *Graph → Hide branches* takes wildcards such as `pipeline/*, release/*`;
    *View → Branch colours…* holds rules such as `feature/*` → purple (first match wins). On
    Apps, hiding `pipeline/*, release/*` takes the graph from 167 to 97 nodes. Only one of the
    64 branches stays: `origin/pipeline/8/15749`, which three prototype and spike branches grow
    out of. Decisions you may want to overrule:
    - **Leaves only, as you asked.** A hidden branch that a shown branch's history contains
      keeps its node and its label. That includes a branch sitting on a commit of `main` that
      never got commits of its own. The alternative would drop such labels too.
    - **Branches only.** Tags, stash and other refs never match (tags have their own toggle).
    - **Matching:** `origin/release/1` matches both `release/*` and `origin/release/*`.
      `*` crosses slashes, `?` is one character, and case doesn't matter.
    - **The current branch** is never hidden, and stays red when a colour rule matches it.
    - **Remote branches get the same colour** as local ones. A paler shade for remotes would
      keep TortoiseGit's local/remote distinction (paler orange against green).
    - **Colours stay as picked in the dark theme.** The built-in colours are
      lightness-inverted there instead.
    - **Global, not per repository**, like the other settings. Both lists start empty.
    - **The Graph menu stays open** when you click inside it, so its text fields can be
      clicked into. Its checkboxes and radio buttons now leave it open too. A click outside
      it or Esc closes it. Other menus still close on any click.
    - **`--hide` and `--branch-color`** replace the saved list or rules, like `--filter`.
      Like every command-line option, the change is saved when the window closes.

## Planned

- [ ] PNG export: SVG exists; TortoiseGit also offers raster formats.
- [ ] Reload automatically when refs change; TortoiseGit only reloads on F5.
- [ ] Tooltip on edges showing the collapsed commits.
- [ ] Less memory for all-commits views of huge repositories (compact adjacency).
- [ ] Windows `.exe` icon resource; try on macOS.
- [ ] Open GitHub PRs in the graph (`docs/research/github-forks-and-pull-requests.md`, §12,
      §14). TortoiseGit has no such feature.
  - [ ] Slice 1: PR-icon tags on nodes whose commit is a PR head, opening the PR in the browser;
        a toolbar toggle, disabled without a GitHub connection; only PRs of `origin` (plus
        the fork's own PRs into its parent) whose base branch is visible. No fetching.
  - [ ] Slice 2: fetch other PR heads commits-only into a private cache; greyed-out nodes,
        dashed edges.

## Done

- [x] License: GPL-3.0-only plus section 7 attribution terms, an About dialog showing them,
      and `THIRD-PARTY-NOTICES.html` (cargo-about) in the CI artifacts.
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
- [x] Native Windows build (MSVC) with a statically linked C runtime; tests, clippy and
      screenshots pass on Windows.
- [x] Terminal output from the Windows release build: attach to the parent console, release it
      before an interactive window opens. `unsafe_code` is `deny` (was `forbid`) so this one
      call can opt out.
- [x] Crossing reduction with transposition and 12 restarts, as OGDF does: 16–30% fewer
      crossings.
- [x] Layout on a background thread, with the view kept anchored on the same commit.
- [x] SVG export from the menu, plus headless `--export out.svg`.
- [x] Filters: current branch only, and a ref-name filter.
- [x] Full commit messages in tooltips, loaded on demand.
- [x] Window icon drawn in code; Linux `.desktop` entry; pre-commit hook (fmt and clippy).
- [x] Hovering an edge lists the commits collapsed into it. Help → Legend explains the colours.
- [x] Independent code review. Fixed:
  - a crash when reloading after deleting a branch or tag
  - loading failures on odd characters in subjects or names
  - lost labels on tags of tags
  - swapped nodes after a reset
  - long edges when splitting rows
  - a race between refs and log during reload
  - debug-build panics on cyclic input

  Its randomised tests are now permanent property tests.
- [x] Demo repository script (`scripts/make-demo-repo.sh`) and a README screenshot.
- [x] Rearranging by hand (question 2):
  - drops become the new resting shape instead of pins
  - drag modes Adapt (springs and weak magnets), Free and Subtree
  - multi-selection with rectangle selection, and "Select subtree"
  - undo and redo; remembered positions per repository
  - edges re-route through the gaps between rows as nodes are moved
- [x] Tag-driven releases (question 12): pushing `vX.Y.Z` builds Linux, Windows and macOS
      archives and publishes a GitHub Release. The build fails unless the tag matches
      `Cargo.toml`. `--version` and the Help menu read `0.3.0 (a1b2c3d)` for releases and
      `0.3.0-dev+a1b2c3d` for every other build.
- [x] Hiding branches by wildcard, leaves only, and colours by branch name (question 13).
      Available in the menus and as `--hide` and `--branch-color`. The status bar counts the
      hidden branches, and the Legend lists the colour rules.
