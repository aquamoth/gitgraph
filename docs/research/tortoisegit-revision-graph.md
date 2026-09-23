# TortoiseGit Revision Graph: how it works

Research notes for re-implementing TortoiseGit's "Revision Graph" as a standalone Rust app.
Everything below comes from reading source code at pinned commits. Where a statement is my own
derivation from the code rather than a quote, it is marked **(derived)**. Things I could not
verify are marked **(unverified)**.

## Sources (pinned)

| Short name | What | Permalink base |
|---|---|---|
| TG | TortoiseGit `master` @ `acc10fc2` (2026-06-27) | https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/ |
| RG | `TG/src/TortoiseProc/RevisionGraph/` | (same base) |
| OGDF | OGDF submodule pinned by TG (`ext/OGDF` → `17f045b1`) | https://github.com/ogdf/ogdf/blob/17f045b131851f5d32af184d5a7a864cec2bfc27/ |
| GIT | upstream git `revision.c` @ `3bc03411` | https://github.com/git/git/blob/3bc0341126508f78f5869cbfc0005e987efdf0c7/revision.c |
| TGIT | TortoiseGit's git fork used by `gitdll` (branch `libgit-gfw-2.50` @ `479ec7e3`) | https://gitlab.com/tortoisegit/tgit/-/blob/479ec7e39b17de6fffb130e224b5965fa37a5d77/ |
| DOC | User manual page | https://tortoisegit.org/docs/tortoisegit/tgit-dug-revgraph.html |

Note: `RevisionGraphWndDraw.cpp`, `src/Utils/Colors.cpp` and `SetColorPage*` do not exist; the real
files are `RevisionGraphDlgDraw.cpp`, `src/TortoiseProc/Colors.cpp`, `Settings/SettingsColors*.cpp`.
The code is a cut-down TortoiseSVN port: TSVN glyph/expand/collapse code sits in `#if 0` blocks and
`m_bTweakTrunkColors`/`m_bTweakTagsColors` are read but never used ([RG/RevisionGraphWnd.cpp#L116-L117, L218-L338](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L218-L338)).

---

## 1. Which commits become nodes

Node selection happens in three stages inside `CRevisionGraphWnd::FetchRevisionData`
([RG/RevisionGraphDlgFunc.cpp#L185-L383](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlgFunc.cpp#L185-L383)).

### Stage A: git's revision walk (`--simplify-by-decoration`)

```cpp
DWORD infomask = CGit::LOG_INFO_SIMPILFY_BY_DECORATION | (m_bShowBranchingsMerges ? CGit::LOG_INFO_SPARSE : 0);
...
if (m_bLocalBranches)      infomask |= LOG_INFO_ALWAYS_APPLY_RANGE | LOG_INFO_LOCAL_BRANCHES; // --branches
else if (m_bCurrentBranch) range += L" HEAD";
else /* all branch */      infomask |= LOG_INFO_ALWAYS_APPLY_RANGE | LOG_INFO_ALL_BRANCH;    // --all
```

`CGit::GetLogCmd` turns this into roughly
`git log -z --all --parents --simplify-by-decoration [--sparse] --topo-order --end-of-options [^from…] [to…] --`
([TG/src/Git/Git.cpp#L1028-L1166](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.cpp#L1028-L1166)).
`--topo-order` comes from the `LogOrderBy` registry default
([TG/src/TortoiseProc/LogDlgHelper.h#L51](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/LogDlgHelper.h#L51)).
The walk runs in-process through `gitdll` (the TGIT fork's `builtin/log.c`/`revision.c`), not by
spawning git.

Git semantics that matter. I checked these in GIT `revision.c`, and the TGIT fork has the same code:
- `--simplify-by-decoration` sets `simplify_merges=1, topo_order=1, rewrite_parents=1, simplify_history=0, prune=1` ([GIT#L2477-L2485](https://github.com/git/git/blob/3bc0341126508f78f5869cbfc0005e987efdf0c7/revision.c#L2477-L2485)).
- A commit counts as "changed" only if it carries a decoration. An undecorated commit is `REV_TREE_SAME` ([GIT#L791-L807](https://github.com/git/git/blob/3bc0341126508f78f5869cbfc0005e987efdf0c7/revision.c#L791-L807)).
- The default decoration set is `HEAD`, `refs/heads/`, `refs/tags/`, `refs/remotes/`, `refs/stash`, `refs/replace/`. It does **not** include `refs/notes/` or `refs/bisect/` ([TGIT refs.c#L90-L131](https://gitlab.com/tortoisegit/tgit/-/blob/479ec7e39b17de6fffb130e224b5965fa37a5d77/refs.c#L90-L131), [TGIT builtin/log.c#L209-L249](https://gitlab.com/tortoisegit/tgit/-/blob/479ec7e39b17de6fffb130e224b5965fa37a5d77/builtin/log.c#L209-L249)). The user's `log.excludeDecoration` / `log.initialDecorationSet` config can change this.
- `--sparse` sets `dense=0`. Then non-merge commits are never TREESAME: `if (!revs->dense && !commit->parents->next) return;` ([GIT#L998](https://github.com/git/git/blob/3bc0341126508f78f5869cbfc0005e987efdf0c7/revision.c#L998)).
- `simplify_merges` then rewrites each parent to its nearest kept ancestor. It drops duplicate parents and parents that are ancestors of another parent. A commit stays only if it is a root, `!TREESAME`, or a merge that still has ≥2 relevant parents. Otherwise it collapses into its single parent ([GIT#L3552-L3662](https://github.com/git/git/blob/3bc0341126508f78f5869cbfc0005e987efdf0c7/revision.c#L3552-L3662)).

**(derived)** What this means:
- **Default mode** (branchings/merges off): nodes are decorated commits (branches, tags, remotes,
  stash, HEAD), root commits, and merge commits whose rewritten parents are still ≥2 independent
  kept commits. Edges go to the nearest kept ancestors. A merge where one rewritten parent is an
  ancestor of the other disappears. Example: a `--no-ff` merge of a deleted branch.
- **Sparse mode** (branchings/merges on): git returns essentially every commit, because all
  non-merges are "changed". Merges survive unless they are redundant. Stage B then collapses the
  result.

### Stage B: TortoiseGit's own linear-chain collapse

This stage runs only when `(!m_bShowAllTags || m_bShowBranchingsMerges)`. With the defaults
(show all tags = on, branchings/merges = off) it is **skipped**. Code:
[RG/RevisionGraphDlgFunc.cpp#L244-L312](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlgFunc.cpp#L244-L312)

```cpp
// keep labeled commits
if (m_HashMap has rev || isSuperRepoHash) {
    if (m_bShowAllTags || isSuperRepoHash) continue;          // any label keeps it
    if (any ref of rev is not ANNOTATED_TAG/TAG) continue;     // tag-only labels do NOT keep it
}
if (rev.m_ParentHash.size() != 1) continue;                    // keep roots & merges
auto childIt = childMap.find(rev.m_CommitHash);
if (childIt == childMap.cend() || childIt->second.size() != 1) continue; // keep tips & fork points
auto& childRev = ...childIt->second[0]...;
if (childRev.m_ParentHash.size() != 1) continue;               // keep direct parents of merges
skipList.insert(rev.m_CommitHash);                             // erase
childRev.m_ParentHash[0] = rev.m_ParentHash[0];                // splice child -> grandparent
```

Loop order is the git output order (topo-order, children first). `childMap` is built once from the
parent lists and patched as commits are removed.

A commit is **kept** if any of these holds:
- It has a ref in `m_HashMap`. When "Show all tags" is off, the ref must be a non-tag ref.
- It is the superproject's recorded submodule commit (`m_submoduleInfo.AnyMatches`).
- It has 0 or ≥2 parents (root or merge).
- It has 0 or ≥2 children in the loaded set (tip or fork point).
- Its only child is a merge.

Everything else is a pure linear pass-through commit and is removed. A commit directly after a
merge or fork point *can* be removed (only its own parent/child counts are checked). `m_HashMap`
has no `HEAD` entry, so a detached HEAD in a linear run is collapsed by stage B even though git
kept it as a decoration **(derived)**.

### Stage C: graph construction

([RG/RevisionGraphDlgFunc.cpp#L314-L359](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlgFunc.cpp#L314-L359))
- One `ogdf` node per remaining commit; one edge per parent link,
  `m_Graph.newEdge(nodes[i] /*child*/, nodes[parent])`, so **edges go child → parent**.
- A parent missing from the result (e.g. cut off by `^from`) gets an extra plain short-hash node.
- `m_HeadNode` (hash == `HEAD`) is used only to scroll there after loading
  ([RG/RevisionGraphWnd.cpp#L1417-L1431](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L1417-L1431)).

**Edge meaning when commits are collapsed:** an edge means "is an ancestor of, through zero or
more hidden commits". The edge has no label and no hidden-commit count.

### Ref map (labels)

`ReloadHashMap()` → `g_Git.GetMapHashToFriendName` iterates **all** references via libgit2
([TG/src/Git/Git.cpp#L2189-L2228](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.cpp#L2189-L2228)):
annotated tags are peeled and stored as `refs/tags/X^{}`; symbolic `refs/remotes/*/HEAD` is
included (so `origin/HEAD` appears as a remote branch); `HEAD` itself is not. Each commit's list is
`std::sort`ed by **full ref name**, which is the row stacking order inside a node:
`refs/bisect/` < `refs/heads/` < `refs/notes/` < `refs/remotes/` < `refs/stash` < `refs/tags/`.

`CGit::GetShortName` strips `refs/heads/`, `refs/remotes/`, `refs/tags/` (and `^{}`) and
classifies each ref as `LOCAL_BRANCH`, `REMOTE_BRANCH`, `ANNOTATED_TAG`, `TAG`, `STASH` (shown as
"stash"), `BISECT_GOOD/BAD/SKIP`, `NOTES` or `UNKNOWN`
([TG/src/Git/Git.cpp#L3016-L3075](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.cpp#L3016-L3075)).

---

## 2. User-facing options

Menu `IDR_REVISIONGRAPH` and toolbar `IDR_REVGRAPHBAR`
([TG/src/Resources/TortoiseProcENG.rc#L3374-L3432](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L3374-L3432)).
Handlers are in [RG/RevisionGraphDlg.cpp](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlg.cpp).

| Menu › item (exact text) | Toolbar? | Effect |
|---|---|---|
| File › "Save graph as..." | – | Export (§7) |
| File › "Exit" | – | Close |
| View › "Zoom in\tCtrl-+" / "Zoom out\tCtrl--" | yes | zoom ÷0.9 / ×0.9, clamped to [0.01, 2.0] |
| View › "Zoom to 100%" | yes | zoom = 1.0 |
| View › "Fit height" / "Fit width" / "Fit graph" | yes | fit to window; never above 2.0 ([L476-L513](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlg.cpp#L476-L513)) |
| (toolbar only) zoom combo | yes | editable; presets `5% 10% 20% 40% 50% 75% 100% 200%`; Enter applies typed value |
| View › "Filter" | yes (checked when any filter is active) | opens the filter dialog (below) |
| View › "Show Overview" | yes | minimap in the bottom-right corner; registry `ShowRevGraphOverview`, default **off** |
| View › "Show branchings and merges" | – | adds `--sparse` + stage-B collapse; registry `ShowRevGraphBranchesMerges`, default **off** |
| View › "Show all tags" | – | when **off**, tag-only commits on linear runs are collapsed; registry `ShowRevGraphAllTags`, default **on** |
| View › "Arrows point towards merges" | – | moves the arrowhead to the child end; registry `ArrowPointToMerges`, default **off** |
| Git › "Compare revisions" / "Unified diff" | – | enabled with 2 selected nodes |
| Git › "Compare HEAD revisions" / "Unified diff of HEAD revisions" | – | enabled with 1 selected node (compares it with `HEAD`) |
| Help › "Help" | – | manual |
| (toolbar) Find | yes | opens the Find dialog (§7) |

Registry defaults: [RG/RevisionGraphDlg.cpp#L256-L259](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlg.cpp#L256-L259).
Every toggle except Overview reruns the whole log and layout (`UpdateFullHistory()`).

**Filter dialog "Revision Graph Filter"**
([TG/…/TortoiseProcENG.rc#L1827-L1843](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L1827-L1843), [RG/RevGraphFilterDlg.cpp](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevGraphFilterDlg.cpp)).
Heading: "Include only the following revision range:".
- **From:** space-separated refs, each passed as `^ref` (autocomplete + "RefBrowser" button).
- **To:** space-separated walk tips replacing `--all`; disabled when either checkbox is ticked.
- **"Only Current Branch"** (walk from `HEAD`) and **"Only Local Branches"** (`--branches`) disable
  each other. **"Reset filter"** clears all. No filter = `--all` (plus any `^from`). Not persisted.

**Settings (Settings › Colors)**: "Note node" colour, "Unknown ref-types" colour, and the checkbox
"Use local branch color for current branch" (registry
`TortoiseProc\Graph\RevGraphUseLocalForCur`). The page "Colors 2" has Current/Local/Remote
branch and Tags colours
([TG/…/TortoiseProcENG.rc#L1412-L1438](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L1412-L1438), [SettingsColors2.cpp](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Settings/SettingsColors2.cpp)).

**Not present in TortoiseGit** (they were TSVN options and are commented out in DOC): orientation
or direction toggles ("oldest on top"), group branches, fold tags, "show all revisions", tree
stripes. There is **no orientation option**. The status bar is created but `UpdateStatusBar` is
commented out, so it stays empty.

---

## 3. Layout (OGDF Sugiyama)

Setup is in the constructor
([RG/RevisionGraphWnd.cpp#L121-L137](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L121-L137)):

```cpp
m_GraphAttr.init(this->m_Graph, ogdf::GraphAttributes::nodeGraphics | ogdf::GraphAttributes::edgeGraphics);
m_SugiyamLayout.setRanking(::new ogdf::OptimalRanking());
m_SugiyamLayout.setCrossMin(::new ogdf::MedianHeuristic());
...
auto pOHL = ::new ogdf::FastHierarchyLayout;
pOHL->layerDistance(30.0);
pOHL->nodeDistance(25.0);
m_SugiyamLayout.setLayout(pOHL);
```

The layout is run once per load with `m_SugiyamLayout.call(m_GraphAttr)`, after node width and
height are set (§4). `m_GraphRect` is `(0,0)–(max x+w/2, max y+h/2)`
([RG/RevisionGraphDlgFunc.cpp#L365-L380](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlgFunc.cpp#L365-L380)).

Parameters TortoiseGit leaves at OGDF defaults
([OGDF SugiyamaLayout.cpp#L843-L874](https://github.com/ogdf/ogdf/blob/17f045b131851f5d32af184d5a7a864cec2bfc27/src/ogdf/layered/SugiyamaLayout.cpp#L843-L874)):
- `runs = 15`, `fails = 4`, `transpose = true`, `permuteFirst = false`.
- `arrangeCCs = true`. Components are packed by `TileToRowsCCPacker` with `pageRatio = 1.0` and
  `minDistCC = LayoutStandards::defaultCCSeparation()`, which is **30.0** in
  [LayoutStandards.cpp#L50-L51](https://github.com/ogdf/ogdf/blob/17f045b131851f5d32af184d5a7a864cec2bfc27/src/ogdf/basic/LayoutStandards.cpp#L50-L51). The header comment says 20.
- `maxThreads = hardware_concurrency`. The crossing-minimisation runs start from random
  permutations seeded by `randomSeed()`, so tie-breaks may vary between runs **(unverified whether
  deterministic in practice)**.

Module details:
- **Ranking: `OptimalRanking`.** Minimises total edge length (min-cost-flow LP). Defaults:
  `separateMultiEdges = true`, acyclic subgraph `DfsAcyclicSubgraph`
  ([OptimalRanking.cpp#L51-L54](https://github.com/ogdf/ogdf/blob/17f045b131851f5d32af184d5a7a864cec2bfc27/src/ogdf/layered/OptimalRanking.cpp#L51-L54)).
  **(derived)** Vertical position is **not** chronological. A short branch's tip sits just one
  layer above its fork point instead of at the top row. Every edge spans ≥1 layer.
- **Crossing minimisation: `MedianHeuristic`**, a layer-by-layer sweep (median), with transpose.
- **Coordinates: `FastHierarchyLayout`** (Buchheim/Jünger/Leipert):
  - `nodeDistance = 25` is the minimum horizontal gap between boxes on one layer.
  - `layerDistance = 30` is the minimum vertical gap between layers.
  - `fixedLayerDistance = false`: the gap grows to `max(30, max|Δx| of edges leaving the layer / 3)`,
    capped at `10*30 = 300`, plus half the box heights ([FastHierarchyLayout.cpp#L963-L982](https://github.com/ogdf/ogdf/blob/17f045b131851f5d32af184d5a7a864cec2bfc27/src/ogdf/layered/FastHierarchyLayout.cpp#L963-L982)).
  - Each layer's height is its tallest box. Nodes are centred on the layer's y.
  - "All edges of the layout will have at most two bends… for each edge having exactly two bends,
    the segment between them is drawn vertically" ([FastHierarchyLayout.h#L44-L77](https://github.com/ogdf/ogdf/blob/17f045b131851f5d32af184d5a7a864cec2bfc27/include/ogdf/layered/FastHierarchyLayout.h#L44-L77)).

**Direction:** edges are child → parent, and the ranking puts an edge's target at a higher rank
than its source. `FastHierarchyLayout` sets `y[layer 0] = height[0]/2` and increases y with each
layer, so layer 0 is at the top (screen y grows downward).
**(derived)** Newest commits (tips) are at the **top** and roots at the **bottom**. Arrows by
default point **down**, towards parents. There is no option to flip this.

**Dummy nodes / bends:** long edges get one dummy node per spanned layer.
`GraphAttributes::transferToOriginal` stores the dummy positions (and any bends) as the original
edge's bend list
([GraphAttributes.cpp#L823-L868](https://github.com/ogdf/ogdf/blob/17f045b131851f5d32af184d5a7a864cec2bfc27/src/ogdf/basic/GraphAttributes.cpp#L823-L868)).
TortoiseGit draws the polyline source centre → bends → target centre (§6).

---

## 4. Node drawing

Code: [RG/RevisionGraphDlgDraw.cpp#L478-L621, L719-L743](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlgDraw.cpp#L478-L743)
and [RG/RevisionGraphWnd.h#L37-L65, L202-L203](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.h#L37-L203).

**Font:** `Gdiplus::Font(CAppUtils::GetLogFontName(), m_nFontSize, FontStyleRegular)`; the log font
defaults to **"Consolas"** ([CommonAppUtils.cpp#L358-L361](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Utils/CommonAppUtils.cpp#L358-L361)).
Size in GDI+ points, **9** at 100%: `m_nFontSize = max(1, int(9*zoom))`, and if `< 6` then
`min(6, int(11*zoom))`
  ([RG/RevisionGraphDlgFunc.cpp#L468-L476](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlgFunc.cpp#L468-L476)).

**Size (logical px at zoom 1)**
```cpp
m_GraphAttr.width(*pnode)  = GetLeftRightMargin() * 2 + xmax;          // margins: 20
m_GraphAttr.height(*pnode) = (GetTopBottomMargin() * 2 + ymax) * lines; // margins: 5
```
`xmax`/`ymax` = largest measured text among `"88888888"` (`GetShortHASHLength()` is hard-coded
to 8) and all ref short names; `lines` = number of refs (min 1) + 1 per submodule label. All rows
of a node share one width.

**Content**
- No refs: only the **8-char short hash**. With refs: **one row per ref** (short name), **no hash**.
- Rows stack top to bottom in the §1 sort order, `rowHeight = nodeHeight / lines`. Only the first
  row rounds its top corners and only the last its bottom ones: one pill split into coloured bands.
- Submodule rows come first: `"super-project-pointer"` and the merge-conflict mine/theirs labels.

**Shape:** GDI+ arcs with a `CORNER_SIZE*zoom = 12*zoom` bounding box, i.e. visible corner radius
**6×zoom** px **(derived)**. `GetNodeRect` trims 1 px of height when > 15 so touching nodes show a gap.

**Colours per row** (`COLORLINE`): fill and 1 px border = ref colour; text **black or white** by WCAG
relative luminance, `L > 0.5 ? Black : White`
  ([L431-L452](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlgDraw.cpp#L431-L452)).
Hash-only rows: border = window colour (invisible), fill = "brightColor" (§5), text black.

**Text placement:** left-aligned at `(x + 20*zoom, y + 5*zoom + rowHeight*row)`.

**HEAD / current branch:** **no dedicated HEAD marker**. Only the local-branch row whose short name
equals `g_Git.GetCurrentBranch()` gets the CurrentBranch colour (LocalBranch colour if
`RevGraphUseLocalForCur` is set). A detached HEAD is not labelled.

**Selection markers** (`DrawMarker`, [L189-L233](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlgDraw.cpp#L189-L233)):
- **Selection 1:** rounded outline, pen `max(1, 4*zoom)`, colour `COLOR_HIGHLIGHT`. A vertical
  tick ("I") is drawn above the box at x+10 from y−25 to y−5 (scaled). The label `"(Base)"` is
  added when a second node is selected.
- **Selection 2:** outline `Color(136,0,21)` and two ticks ("II") at x+5 and x+15.

---

## 5. Colours

Defaults from [TG/src/TortoiseProc/Colors.cpp#L23-L53](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Colors.cpp#L23-L53)
(registry `HKCU\Software\TortoiseGit\Colors\<Name>`). They are mapped to ref types in
`SetupColorsAndBrushes` / `DrawTexts`
([RG/RevisionGraphDlgDraw.cpp#L454-L476, L568-L604](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlgDraw.cpp#L454-L604)).

| Row kind | Colour key | Default RGB | Hex | Text (derived via luminance) |
|---|---|---|---|---|
| Current branch (local, = current) | `CurrentBranch` | (200, 0, 0) | `#C80000` | white |
| Local branch | `LocalBranch` | (0, 195, 0) | `#00C300` | white (L≈0.39) |
| Remote branch (incl. `origin/HEAD`) | `RemoteBranch` | (255, 221, 170) | `#FFDDAA` | black |
| Tag, annotated **and** lightweight (same colour) | `Tag` | (255, 255, 0) | `#FFFF00` | black |
| Stash | `Stash` | (128, 128, 128) | `#808080` | white |
| Notes (`refs/notes/*`) | `NoteNode` | (160, 160, 0) | `#A0A000` | white |
| Bisect good | `BisectGood` | (0, 100, 200) | `#0064C8` | white |
| Bisect bad | `BisectBad` | (255, 0, 0) | `#FF0000` | white |
| Bisect skip | *uses `BisectBad`* (bug, L471) | (255, 0, 0) | `#FF0000` | white |
| Other/unknown refs | `OtherRef` | (224, 224, 224) | `#E0E0E0` | black |
| Submodule super-project pointer | hard-coded | (246, 153, 253) | `#F699FD` | white (L≈0.495) |
| Plain commit (no refs) fill | computed "brightColor" | ARGB(229, 229, 229, 255) | ≈`#E8E8FF` on white | black |
| Plain commit border | `GetSysColor(COLOR_WINDOW)` | usually (255,255,255) | – | – |
| Canvas background | themed `COLOR_WINDOW` | usually white | – | – |
| Edges + arrowheads | themed `COLOR_WINDOWTEXT` (SVG export: black) | usually (0,0,0) | – | – |

Notes:
- **Plain commit fill.** The code is
  `LimitedScaleColor(background, RGB(255, 0, 0), 0.9f)`. `RGB(255,0,0)` is a COLORREF
  (`0x000000FF`) implicitly converted to `Gdiplus::Color(ARGB)`, which is *transparent blue*
  (A=0,R=0,G=0,B=255). Per channel, `min(c1, c2 + (c1-c2)*0.9)` gives ARGB(229,229,229,255).
  Over white this renders as roughly `#E8E8FF`, a very light lavender **(derived; not visually
  confirmed)**.
- **Registry key bug.** `BisectSkip` shares the registry key `Colors\BisectBad` (default
  (192,192,192)) in `Colors.cpp`. The graph ignores it anyway.
- **Dark mode.** Ref colours pass through `CTheme::GetThemeColor(c, true)`, which inverts HSL
  lightness (`l = 100 - l`, clamped to [5, 90]) when the dark theme is on
  ([TG/src/Utils/Theme.cpp#L141-L160](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Utils/Theme.cpp#L141-L160)).
- **Not used by the graph:** `BranchLine1..8` (log-list lane colours) and `LastCommitNode`.

---

## 6. Edge drawing

Code: [RG/RevisionGraphDlgDraw.cpp#L235-L399](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlgDraw.cpp#L235-L399)

- **Geometry: straight-segment polylines** (`Graphics::DrawLines`), no curves.
  - The points are: source centre, then every OGDF bend point in order, then target centre.
  - The first and last points are clipped to the node boxes by `cutPoint` (line/rectangle
    intersection, box inflated by `lw/2 = 0.5`).
  - Anti-aliased.
- **Pen:** themed `COLOR_WINDOWTEXT`, width `max(1, 2*zoom)`. Every edge has the same colour; no
  per-branch colouring.
- **Arrowhead:** one per edge. By default it sits at the **target = parent** end, pointing towards
  the older commit. With "Arrows point towards merges" it sits at the **source = child** end.
  - Shape: a closed 5-point path `notch → wing1 → tip → wing2 → notch`.
  - Tip = clipped endpoint. Wings are `8*zoom` long at ±π/8 (22.5°) from the edge direction.
    The notch is 0.6×8×zoom back along the edge.
  - GDI+ draws it with `DrawPath` (outline only, same 2 px pen). SVG export fills it black.
- **Paint order:** nodes (`DrawTexts`) first, then edges (`DrawConnections`), so an edge crossing a
  box is drawn on top ([L660-L661](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlgDraw.cpp#L660-L661)).
- **Routing:** no custom routing. Bends come only from OGDF dummy nodes, so there is at most one
  bend near each end and a vertical middle segment for long edges (see §3).

---

## 7. Interaction

Mouse/keyboard code: [RG/RevisionGraphWnd.cpp#L444-L1361](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L444-L1361).

**Mouse and keyboard**
- **Click a node:** it becomes selection #1 (clicking #1 again deselects it) and #2 is cleared.
  **Ctrl+click** toggles a second selection (clicking #1 promotes #2 to #1; a third node replaces #2).
  **Click empty space:** clears the selection and starts **drag-to-pan**. Hit testing is a linear
  scan over node rectangles. The Git menu items are enabled from the selection count.
- **Double-click:** none (no `ON_WM_LBUTTONDBLCLK`, although the window class has `CS_DBLCLKS`).
- **Wheel** scrolls vertically by `zDelta` px, **Shift+wheel** horizontally. **Ctrl+wheel** zooms
  ×0.9/÷0.9 per notch, clamped to [0.01, 2.0], without updating the zoom combo.
- **Keys:** arrows scroll 20 px, PgUp/PgDn half the graph height, **F5** reloads, **Ctrl+F** opens
  Find. The accelerator table is commented out
  ([RG/RevisionGraphDlg.cpp#L261, L377-L432](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlg.cpp#L377-L432)),
  so the "Ctrl-+ / Ctrl--" shown in the menu probably do nothing **(unverified at runtime)**.
- Leftover TSVN rubber-band zoom in `OnLButtonUp` needs a ≥20 px delta since the last mouse move;
  pan updates that anchor on every move, so it effectively never fires **(derived)**.

**Tooltip** (hover over a node, [L786-L809](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L786-L809)):
```
<full 40-hex hash>
<Author Name> <<email>> YYYY-MM-DD HH:MM      (author date)

<subject>
<body>
```
Truncated at 8000 chars, fitted to the screen area beside the cursor, shown for 32.767 s. No refs.

**Context menu** (right-click; [L1120-L1315](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L1120-L1315)).
With 0–1 selected, right-clicking a node makes it selection #1. With 2 selected, the menu opens only
over one of them. Separators are inserted between ID groups.
- **One node:**
  1. "Show log", "Browse repository"
  2. `Switch/Checkout to "<branch>"` (exactly one non-current local branch), a "Switch/Checkout to"
     submenu (several), or else "Switch/Checkout to this..." (first remote branch/tag)
  3. "Copy ref names" (full names, one per line, or the hash if there are none)
  4. `Delete <ref>`, or a "Delete branch/tag" submenu listing each ref + "All" (current branch
     excluded)
  5. "Compare HEAD revisions", "Unified diff of HEAD revisions", "Compare with working tree"
- **Two nodes:** "Show log" (range) | "Compare revisions", "Unified diff". Holding Shift selects the
  alternative diff tool (`/alternative`).

**Find** (Ctrl+F/toolbar; [RG/RevisionGraphDlg.cpp#L515-L613](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlg.cpp#L515-L613)):
- Modeless "Find" dialog. "Full text search" (Match case, Regular Expression) covers subject,
  message, author, email, hash and ref names; it cycles from the last hit and wraps with a
  flash/beep. The "Ref (Click it then go to)" list resolves `ref^{}` and jumps to it.
- A hit is scrolled into view (≈25 px below the top edge) and selected; Shift scrolls without
  selecting. Only commits that are nodes can be found.

**Overview / minimap:** a bitmap of the whole graph in the **bottom-right** corner, sized
`max(100, clientW/4) × max(200, clientH/4)` (DPI-scaled) at zoom ≤ 1, skipped above 10,000 nodes.
The viewport is a translucent black rectangle (alpha 64); click/drag inside the minimap scrolls.

**Initial view:** after loading, the view scrolls so the HEAD node is at the top-left. The canvas
shows "Loading..." during the fetch and "No graph available" for an empty graph.

**Export** ("Save graph as...", [L811-L956](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L811-L956)):
`.svg` (default), `.gv` (Graphviz), `.wmf`, or raster `.png/.jpg/.jpeg/.bmp/.gif`. Vector formats
are rendered at 100%, raster at the current zoom. `TortoiseGitProc /command:revisiongraph
/output:<file>` renders hidden, saves and exits
([RevisionGraphCommand.cpp](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Commands/RevisionGraphCommand.cpp)).

---

## Quirks worth not copying (or copying on purpose)

- Node sizes are measured with the **current zoom's** font size during the fetch. Pressing F5
  while zoomed produces boxes that don't match the text at other zooms **(derived)**.
- The default mode (git `--simplify-by-decoration`) can hide `--no-ff` merges and merges of deleted
  branches, because git's merge simplification drops redundant parents **(derived from git
  source; worth an empirical test against a real repo)**.
- No lane/branch colouring of edges, no commit dates on the axis, and no count of collapsed
  commits on edges.
- The user manual (DOC) is thin. Its only concrete statements are that the graph shows
  ref-pointed commits, tooltips show date/author/message, Ctrl-click selects two, the overview is
  draggable, and F5 refreshes.
