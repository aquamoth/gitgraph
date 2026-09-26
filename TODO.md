# TODO

## Open questions (HITL)

_Decisions I made on my own that you may want to overrule. Try them with `parterre` on
`~/Source/repos/Cosmo/Apps`; most are one click in the toolbar or menus._

_Numbers are never changed or reused, even after an item is deleted. Next number: 22._

1. **Default look: "Modern" or "Classic"?** *Settings → Appearance → Style* switches.
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
   - **No blue dots** any more (your notes of 2026-09-24, third round). Nodes you moved can
     still be returned to the layout (right-click, or `R` for all).
   - **Magnets act between nodes.** During a drag, edges make way only for nodes in their own
     row; once the node is dropped on them they re-route around it.
   - **Which edges re-route.** Edges at nodes you moved re-route as soon as they change.
     Edges that only gave way keep the layout's route (bent along) unless pulled more than
     40 px out of shape. Re-routing those too made whole fans lose their bundled trunks when a
     shared parent moved a pixel.
   - **Re-routed edges switch at once,** without animating from the old route to the new.
     They also leave the trunks that bundled edges share.
   - **Reversed edges** (third round, which overrules the second): edges always leave the
     child's bottom and enter the parent's top. An edge whose parent is dragged above its
     child is routed round both nodes, from just below the child to just above the parent,
     so the reversal shows as a loop.
   - **Children above parents** (third round). In Adapt, dragging a node up pushes its
     children up ahead of it, and dragging it down pushes its parents down, with at least
     24 px between boxes. Only the dragged nodes can end up past a parent. Free and Subtree
     move nothing else, so there the order can break, and a reversal left at rest is kept
     when the graph later adapts around it. OK?
   - **Free and Subtree allow overlaps,** and Adapt leaves them alone: it only keeps nodes
     from coming closer than they rest.
   - **Defaults:** pull 0.3 (a neighbour moves about half as far as the dragged node, the next
     one a quarter), push 0.5 (nodes start pushing each other 32 px apart), wobble 0.4. *Pull*
     replaces the old *reach* setting and starts at its new default.
   - **The old prototypes are gone.** Spider web and Strings became Adapt; Rigid became Free.
   - **Mode switching** uses keys and buttons only. Shift and Ctrl already mean selection;
     another modifier (such as holding Space) could give a one-off Free drag.
   - **Remembering moves.** *Remember moved nodes* (in the drag options, off by default) keeps
     nodes where they rest, per repository, across runs and relayouts. Should it be on by
     default?
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
   Tune it under *Settings → Advanced*. Happy with the default?
6. **Stash** is shown, as in TortoiseGit, as a single edge to its base commit. The index and
   untracked-files snapshot commits are hidden.
7. **`origin/HEAD`**-style symbolic refs are hidden, because they duplicate `origin/main`.
   TortoiseGit shows them.
8. **Other refs** (`refs/t3/*` in Apps) are hidden by default; ☰ → Show → Other refs shows them.
9. **HEAD marker.** Like TortoiseGit, only the current branch's row is highlighted (red). A
   detached HEAD gets its own red "HEAD" row, which TortoiseGit doesn't have.
10. **No git actions.** Answered 2026-09-26: yes, towards parity with TortoiseGit's node menu.
    The roadmap, its boundary rule and the open decisions live in the map *Revision-graph node
    menu: roadmap to TortoiseGit parity* ([#25](https://github.com/aquamoth/parterre/issues/25)).
    Show log comes first; *Browse repository* and the menu-bar Git menu are out.
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
    - **Version in the UI:** besides `--version`, the ☰ menu ends with a greyed
      `parterre 0.3.0 (a1b2c3d)` line, for users who start parterre from a file manager or
      Start menu and never see a terminal.
    - **Assets:** one archive per target (`.tar.gz`, `.zip` on Windows) holding the binary,
      the README, `LICENSE`, `NOTICE` and `THIRD-PARTY-NOTICES.html`, plus `SHA256SUMS`. macOS
      gets both Apple silicon and Intel builds, the Intel one cross-compiled and therefore not
      test-run in the workflow.
    - **Linux baseline** (your decision of 2026-09-25): built in an Ubuntu 22.04 container, so
      the binary needs glibc 2.35 or newer (Debian 12, Ubuntu 22.04 and later).
    - **Commit detection** is a `build.rs` running `git`, with no dependencies. Without git it
      falls back to a bare `X.Y.Z-dev`. A clean build of exactly the released sources shows
      the plain version (your decision of 2026-09-25): the release workflow, a clean checkout
      of the tag, or the crate from crates.io, whose commit comes from `.cargo_vcs_info.json`.
    - **Dirty** means uncommitted changes under `crates/`, `.cargo/`, the Cargo files or
      `rust-toolchain.toml`, the files that go into the binary. Edits to docs don't count.
13. **Hiding and colouring branches by name** (your request of 2026-09-25). Neither is in
    TortoiseGit. *Hide branches* (the toolbar's filter options, or *Settings → Filters*) takes
    wildcards such as `pipeline/*, release/*`; *Settings → Branch colours* holds rules such as
    `feature/*` → purple (first match wins). On
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
    - **The filter options stay open** when you click inside them, so their text fields can
      be clicked into. A click outside or Esc closes them. Menus close on any click.
    - **`--hide` and `--branch-color`** replace the saved list or rules, like `--filter`.
      Like every command-line option, the change is saved when the window closes.
14. **Edge ends in Classic.** On your request, edges now always leave a node's bottom centre
    (towards its parents) and enter the top centre (from its children), in both looks, and
    arrowheads are 13 px instead of TortoiseGit's 8. TortoiseGit instead clips each edge where
    it meets the box border, so edges can end on any side. Should Classic keep TortoiseGit's
    clipping?
15. **App icon.** Decided 2026-09-25: the revision graph planted as a parterre, seen from
    above, on the dark theme's slate (variant C1). The prototype with every candidate, the
    verdicts and a head-to-head of the last two is on the branch `prototype/app-icon`
    (`packaging/icon-prototype/index.html`). Nothing is taken from the publisher's name. macOS
    26 could also take a dark appearance; nothing else can, so one icon serves everywhere.
16. **Toolbar, menu and settings** (reorganised 2026-09-26 after the prototype on the branch
    `prototype/menus`). Calls I made that the prototype didn't settle:
    - **Left out of the ☰ menu:** "Select subtree of selection" and "Return selection to
      layout". The right-click menu has both, and acts on the selection the node belongs to.
    - **Added to the ☰ menu:** *Find* (`Ctrl+F`) in the toolbar's order, and the version at
      the foot (question 12).
    - **Only in the ☰ menu and *Settings → Graph*:** stash, other refs and "tags make nodes".
      The toolbar's filter options keep the four filters you change most.
    - **Physics sliders are always enabled** (*Settings → Advanced*). They only affect Adapt,
      as their tooltips say; before, they were greyed out in the other modes.
    - **The toolbar no longer wraps.** In narrow windows the find field shrinks instead, and
      drops its `Ctrl+F` hint.
17. **Windows installer** (#15, `docs/building.md` → "Windows installer"). Tested on Windows 11:
    per-user and machine-wide installs, uninstalls, upgrades, same-version upgrades and a
    refused downgrade. Calls #15 didn't settle:
    - **MSI rather than MSIX** (your question of 2026-09-26). MSIX must be signed, and winget
      refuses unsigned ones; it installs per-user only, so Chocolatey's machine-wide install
      has no counterpart; and an Explorer entry (#11) would need a COM shell extension instead
      of a few registry keys. MSIX would bring clean sandboxed uninstalls and Store updates.
      Worth another look only with code signing.
    - **No installer UI.** A plain MSI shows only a progress bar, which suits winget and
      Chocolatey. Someone downloading it from GitHub sees no welcome or finish page. Adding one
      takes WiX's `WixToolset.UI.wixext` extension.
    - **Registry key** `Software\Trustfall AB\parterre` (in HKCU or HKLM), used only as the
      components' key paths, which Windows Installer needs under a user's profile.
    - **The Start menu entry** opens an empty window that asks for a repository, since #12
      (question 18). Before, started outside a repository, parterre showed nothing.
    - **Two ICE checks are suppressed:** ICE57, which doesn't understand dual-purpose packages,
      and ICE61, which warns about the same-version upgrades we want.
    - **Per-user and machine-wide don't replace each other.** Windows Installer only upgrades
      within one scope, so a user who installs per-user and later machine-wide (or the other
      way round) gets two entries in *Settings → Apps*. Known MSI behaviour, not tested.
18. **Opening folders** (your request of 2026-09-26). Without a path, parterre opens the
    current directory's repository, or else an empty window asking for one. The ☰ menu starts
    with *Open folder…* (`Ctrl+O`), *Recent folders* and *Close folder* (`Ctrl+W`), not in
    the toolbar. Decisions you may want to overrule:
    - **A path given that is not a repository** still ends with an error in a terminal, as
      before. Without one (Explorer's menu, a shortcut, a desktop entry) the empty window
      opens and shows the error instead, since #11 (question 21).
    - **The empty window also has an *Open folder…* button and the five most recent
      folders.** That is more than the message you asked for; the menu has the same.
    - **Recent folders:** the ten newest, each shown by name with the folder it is in (two
      `Apps` repositories stay apart). The open one is left out. The list also takes
      repositories opened from the command line or the current directory.
    - **A recent folder that fails to open leaves the list**, with the reason in the status
      bar, so that deleted repositories don't linger. A drive that is only unplugged loses
      its entries too.
    - **The folder picker** is the platform's own: Windows' dialog, macOS's, and on Linux the
      XDG desktop portal, or zenity where there is no portal (the `rfd` crate, without GTK).
      It starts in the folder around the open or most recent repository. Any folder inside a
      repository opens that repository.
    - **Items that need a repository** (undo, reload, export, close) are greyed out while none
      is open. The toolbar stays as it is.
19. **Log window layouts** (#40). #29 didn't settle:
    - **The picker** is four icon segments drawing each layout's panes, named in their
      tooltips, like the toolbar's drag modes. No keyboard shortcut for switching (the
      prototype's ← → keys were prototype chrome).
    - **Reset** is an icon button right of the picker. It moves only the current layout's
      dividers back, and is greyed out while they are where they start.
    - **In the settings** the same picker is a row "Layout" under a heading "Log window" at the
      bottom of *Appearance* (below the fold: the page scrolls), with the layout's name beside
      it. A page of its own for one row seemed too much.
    - **Smallest pane:** 8 % of the height, 15 % of the width (the panes are tables, which need
      room across). Starting positions are the prototype's.
    - **Narrow panes get narrower columns:** a commit list under 720 points wide gets the
      prototype's narrower author and date columns (as in its layouts B and D), and a
      changed-files table whose path would get under 260 points gets narrower columns with
      shorter headings ("Ext.", "Added", "Removed"; the full name in the tooltip). Decided by
      width, not by layout, so a narrow window in layout A gets them too.
20. **Log window** (#39, layout A). Calls #27–#29 didn't settle:
    - **Ref badges follow the graph's ref kinds.** The log shows badges (and names the range
      with refs) only of the kinds the graph shows: hide remote branches, other refs or the
      stash in the graph and they go from the log too. The alternative is every ref, always,
      which on Apps would add the `refs/t3/*` checkpoints.
    - **Esc in the filter field** only leaves the field; a second Esc closes the window.
    - **F5 in the log window** reloads the whole repository, graph included, as F5 in the
      graph does; the log re-runs its query and keeps the selected commit.
    - **Show log while the window is open** replaces its contents and asks the window manager
      to raise it (Wayland may ignore that). Sort and filter of the changed files, and the
      divider positions, carry over to the new log.
    - **Size on first open:** 1100 × 760; after that, the size it last had.
    - **No keyboard focus for the list:** the arrow keys, Page Up/Down, Home and End move the
      selected commit whenever the filter field doesn't have the keyboard.
21. **Explorer context menu** (#11). *Revision Graph* on a folder and on the
    background of an open one, registered by the MSI. Clicked through in Explorer on a folder
    (it opened the graph); on a folder's background only its keys and command were checked.
    Named *Revision Graph*, without the "(parterre)" #11 had, on your call of 2026-09-26: the
    icon says whose it is. Calls #11 didn't settle:
    - **Windows 11 shows it under *Show more options*.** At the top level it would need an
      `IExplorerCommand` handler and package identity (MSIX or a sparse package), as #11 said.
    - **Every folder gets the entry**, in or out of a repository: a plain registry verb
      can't ask git. Outside one, parterre opens with the error and *Open folder…*.
      TortoiseGit's shell extension can hide it; that takes a COM handler.
    - **Errors in a window only when there is no terminal** (stderr isn't one). From a
      terminal a bad path still prints the error and exits, and so does `--export`. Run with
      stderr redirected to a file, a bad path now opens the window too; `--screenshot` runs
      still fail. Other startup failures (no OpenGL, as over some remote desktops, or a
      panic) still reach only stderr, so from Explorer nothing would show.
    - **One folder at a time:** with several folders selected the entry is missing, rather
      than opening a window for each.
    - **Not on drives** (`Drive\shell`): right-clicking `C:` in *This PC* has no entry; the
      background of an open drive does. Easy to add if repositories at a drive's root matter.
    - **Not optional:** every install gets it; the MSI has no UI to leave it out.
    - **Linux:** not done. `MimeType=inode/directory` in the desktop entry would list
      parterre under *Open With* for folders, but some desktops then make it the default
      folder handler (VS Code had that bug), so it needs trying on GNOME and KDE first.

## Planned

- [x] Show log window, as planned in the map *Revision-graph node menu: roadmap to TortoiseGit
      parity* ([#25](https://github.com/aquamoth/parterre/issues/25)). Deliberate deviation
      from TortoiseGit (decided in #28): when the second of two selected nodes is an ancestor
      of the first, the two are swapped instead of showing an empty list.
  - [x] Layout A (stacked) and its entry points: *Show log* first in the node menu, `L` and
        double-click ([#39](https://github.com/aquamoth/parterre/issues/39); see question 20).
  - [x] Layouts B, C and D, the layout picker (in the window's header and in *Settings →
        Appearance*) and reset, and the layout and divider positions per layout saved with the
        settings ([#40](https://github.com/aquamoth/parterre/issues/40); see question 19).
- [x] Wayland freeze ([#38](https://github.com/aquamoth/parterre/issues/38)). On Wayland the
      whole app froze when one of its windows was minimized while another was open; it
      happened with Settings already. Worked around (see `frame_pacing.rs`, and
      `docs/research/wayland-viewport-freeze.md` on the branch
      `research/wayland-viewport-freeze`): on Wayland only, vsync off and frames capped at about
      8 ms. Not verified on Wayland after the change (no headless Wayland to test with).
  - [ ] **Check regularly, and on every eframe upgrade, whether the upstream fix has shipped:**
        <https://github.com/emilk/egui/pull/8631> (bug:
        <https://github.com/emilk/egui/issues/5145>). Once it is in a released eframe, remove
        the frame cap and turn vsync back on.
- [ ] Short hashes in the graph as long as git makes them for the repository (`core.abbrev`
      auto: 9 on Apps), as the log window will. Today the graph uses a fixed 8, and 10 in one
      place.
- [x] Settings window without minimize and maximize buttons. It is a dialog, and maximizing it
      breaks its layout. Asked of winit, which (0.30) does this on Windows and macOS only. On
      Linux it ignores the request: there the window loses only its maximize button, because it
      can't be resized (winit's own Wayland title bar, as on GNOME, leaves maximize out, and X11
      window managers get a "not maximizable" hint), and minimize stays. Not checked by hand on
      any platform.
- [ ] Toolbar merged into the title bar, with ☰, the repository name and the window buttons in
      one row (wanted 2026-09-26, postponed as too big a change for now). Native on macOS
      (content under a transparent title bar, the traffic lights stay). Elsewhere parterre
      would draw its own title bar: moving, resizing and double-click to maximise by hand; no
      Windows 11 snap-layout popup; on GNOME no compositor shadow. See the "Title bar: merged"
      toggle in the prototype on the branch `prototype/menus`.
- [ ] PNG export: SVG exists; TortoiseGit also offers raster formats.
- [ ] Reload automatically when refs change; TortoiseGit only reloads on F5.
- [ ] Tooltip on edges showing the collapsed commits.
- [ ] Less memory for all-commits views of huge repositories (compact adjacency).
- [ ] Distribution, as decided in `docs/distribution.md`: crates.io (#13), Windows MSI (#15),
      winget (#16), Chocolatey (#17), .deb and .rpm (#18), Snap (#19), publishing behind one
      approval (#20), Flathub later (#21). The MSI (#15) is built by CI and attached to
      releases, and parterre finds Git for Windows when git isn't on PATH (question 17).
- [x] Explorer context menu (#11; see question 21).
- [ ] macOS `.app` bundle, so the Dock shows `packaging/icon/parterre.icns`; the release ships
      a bare binary, which gets the generic icon.
- [ ] After some sequences of drags, undo and redo, a reset leaves an edge with a route of
      its own. `physics_random_drags` finds one with seed 31337 (iteration 247), on main before
      the child-above-parent ordering was merged too.
- [ ] Open GitHub PRs in the graph (`docs/research/github-forks-and-pull-requests.md`, §12,
      §14). TortoiseGit has no such feature.
  - [ ] Slice 1: PR-icon tags on nodes whose commit is a PR head, opening the PR in the browser;
        a toolbar toggle, disabled without a GitHub connection; only PRs of `origin` (plus
        the fork's own PRs into its parent) whose base branch is visible. No fetching.
  - [ ] Slice 2: fetch other PR heads commits-only into a private cache; greyed-out nodes,
        dashed edges.
- [ ] Menus that overflow the window on Windows, like TortoiseGit's native ones: each menu (and
      submenu) as a borderless egui viewport placed in screen coordinates, kept on the monitor
      by sliding up from its bottom edge. Needs a Windows agent to build and try it; watch
      for the main window losing focus while a menu is open, and find the monitor's work area
      for multi-monitor setups. Hover-to-open submenus, closing on outside clicks and the
      keyboard then work across windows, so egui's menu logic has to be redone. For now menus
      stay inside the window and scroll (`menu::fit_window`). Not planned: Wayland (winit 0.30
      has no xdg_popup, and a client can't place its own windows), macOS (no agent to test
      on), X11 (possible with override-redirect windows, but few users).

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
- [x] App icon (`parterre-core::icon`): the window icon, the SVG, PNGs, the `.ico` embedded in
      the Windows `.exe` and the `.icns` are all generated from one drawing.
- [x] Version information in the Windows `.exe` (*Properties → Details*): product name,
      versions, copyright and Trustfall AB as the company.
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
      `Cargo.toml`. `--version` and the ☰ menu read `0.3.0 (a1b2c3d)` for releases and
      `0.3.0-dev+a1b2c3d` for every other build.
- [x] Hiding branches by wildcard, leaves only, and colours by branch name (question 13).
      Available in the menus and as `--hide` and `--branch-color`. The status bar counts the
      hidden branches, and the Legend lists the colour rules.
- [x] Direction made visible:
  - edges leave the bottom of a node and enter the top; an edge turned around loops round its
    nodes
  - bigger arrowheads
  - Adapt keeps children above parents (about 1 ms more per frame with 8000 particles awake)
  - click an edge to keep it highlighted, also in the overview; the status bar says where it
    leads
  - the blue dot is gone
- [x] Context menu restyled after current desktop menus: rounded, soft shadow, roomier rows
      with a rounded highlight, shortcuts on the right, unavailable items greyed out rather
      than left out. "Follow system" now follows the desktop's light or dark mode on Linux
      too, switching as soon as the desktop does (XDG desktop portal, via `gdbus`); winit
      reports no system theme there, so it used to be dark always.
- [x] Title bar: on GNOME (Wayland desktops that leave it to the app) winit's Adwaita-style
      title bar with the window title and round buttons, instead of a plain dark bar. The
      title bar follows parterre's light or dark theme, also on Windows and macOS.
- [x] Opening and closing folders from the ☰ menu, with recent folders (#12, question 18).
      Without a path, the current directory's repository or an empty window that asks for one.
- [x] Toolbar, ☰ menu and settings reorganised (question 16): icon tools for what to show, the
      ref toggles with filter options, find, zoom, HEAD, the overview map and the drag modes
      with their options; everything again in the ☰ menu, in the toolbar's order; the rest in
      a settings window that leaves the graph visible and applies changes at once. The status
      bar can be hidden, and no longer shows the layout time or the drag mode's description.
- [x] A menu or popover taller than the window scrolls, with a visible thin scroll bar, instead
      of being cut off at the bottom; one that fits but not below its button slides up to the
      window's bottom edge, as before.
